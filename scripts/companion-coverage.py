#!/usr/bin/env python3
"""Gate Windows companion transport, trust and local operator modules at 71% line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def run(arguments, root, environment=None):
    result = subprocess.run([str(item) for item in arguments], cwd=root, env=environment,
                            text=True, encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        print(result.stdout, end="", file=sys.stderr)
        print(result.stderr, end="", file=sys.stderr)
        raise subprocess.CalledProcessError(result.returncode, arguments)
    return result.stdout


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=root / ".research/companion-coverage")
    parser.add_argument("--service-only", action="store_true", help="Measure the portable crate without desktop IPC")
    options = parser.parse_args()
    output = options.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    environment = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(output / "target")),
                       RUSTFLAGS="-C instrument-coverage", LLVM_PROFILE_FILE=str(profiles / "build-%m-%p.profraw"))
    suffix = ".exe" if os.name == "nt" else ""
    sysroot = Path(run(["rustc", "--print", "sysroot"], root).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"], root), re.M).group(1)
    llvm = sysroot / "lib/rustlib" / host / "bin"
    for tool in ["llvm-profdata", "llvm-cov"]:
        if not (llvm / f"{tool}{suffix}").exists():
            raise SystemExit("Install llvm-tools-preview for the pinned Rust toolchain.")
    targets = [("service", root / "crates/companion-service/Cargo.toml", "", [root / f"crates/companion-service/src/{name}.rs" for name in ["lib", "sources", "persistence", "publication", "render_object", "feedback", "hub", "hub_feedback", "hub_assignments", "tls", "server", "client"]])]
    if not options.service_only:
        targets.append(("desktop", root / "src-tauri/Cargo.toml", "companion_host::tests", [root / "src-tauri/src/companion_host.rs"]))
    summary = {}
    for name, manifest, selection, sources in targets:
        artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json", "--manifest-path", manifest], root, environment)
        binaries = [item["executable"] for line in artifacts.splitlines()
                    if (item := json.loads(line)).get("reason") == "compiler-artifact"
                    and item.get("executable") and item.get("profile", {}).get("test")]
        if len(binaries) != 1:
            raise RuntimeError(f"Expected one {name} library test executable.")
        test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / f"{name}-%m-%p.profraw"))
        arguments = [binaries[0], *([selection] if selection else [])]
        tests = subprocess.Popen(arguments, cwd=root, env=test_environment, text=True, encoding="utf-8",
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        log, _ = tests.communicate()
        (output / f"{name}-tests.log").write_text(log, encoding="utf-8")
        if tests.returncode:
            print(log, flush=True)
            raise subprocess.CalledProcessError(tests.returncode, arguments)
        raw_profiles = list(profiles.glob(f"{name}-*-{tests.pid}.profraw"))
        if not raw_profiles:
            raise RuntimeError(f"The {name} process produced no coverage evidence.")
        merged = output / f"{name}.profdata"
        run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *raw_profiles, "-o", merged], root)
        for source in sources:
            annotation = run([llvm / f"llvm-cov{suffix}", "show", binaries[0], f"-instr-profile={merged}", "-show-line-counts-or-regions", source], root)
            (output / f"{name}-{source.stem}-lines.txt").write_text(annotation, encoding="utf-8")
            lines = source.read_text(encoding="utf-8").splitlines()
            # lib.rs declares test modules but has production exports/functions afterward.
            end = len(lines) + 1 if source.name == "lib.rs" else next((index + 1 for index, line in enumerate(lines) if line == "#[cfg(test)]"), len(lines) + 1)
            counts = {int(match[1]): int(match[2]) for line in annotation.splitlines()
                      if (match := re.match(r"^\s*(\d+)\|\s*(\d+)\|", line)) and int(match[1]) < end}
            if not counts:
                raise RuntimeError(f"No instrumented production lines for {source}.")
            covered = sum(count > 0 for count in counts.values())
            percent = 100 * covered / len(counts)
            key = f"{name}/{source.name}"
            summary[key] = {"covered": covered, "lines": len(counts), "percent": round(percent, 2)}
            print(f"{key}: {covered}/{len(counts)} production lines ({percent:.2f}%)", flush=True)
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(item["percent"] < 71 for item in summary.values()):
        raise SystemExit("Each companion production module must meet 71% line coverage.")


if __name__ == "__main__":
    main()
