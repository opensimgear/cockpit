#!/usr/bin/env python3
"""Measure portable desktop-output modules without constructing a native injector."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(arguments, root, env=None):
    result = subprocess.run([str(arg) for arg in arguments], cwd=root, env=env,
                            capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        print(result.stdout + result.stderr, flush=True)
        result.check_returncode()
    return result.stdout


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=root / ".research/desktop-output-coverage")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    temporary = output / "test-tmp"
    temporary.mkdir(exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / "target"),
               RUSTFLAGS="-C instrument-coverage", TEMP=str(temporary), TMP=str(temporary),
               LLVM_PROFILE_FILE=str(profiles / "%m-%p.profraw"))
    artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json",
                     "--manifest-path", root / "crates/desktop-output/Cargo.toml"], root, env)
    binaries = [entry["executable"] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry.get("executable") and entry.get("profile", {}).get("test")]
    if len(binaries) != 1:
        raise RuntimeError("Expected one desktop-output library test executable.")
    (output / "tests.log").write_text(run([binaries[0]], root, env), encoding="utf-8")
    sysroot = Path(run(["rustc", "--print", "sysroot"], root).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"], root), re.M).group(1)
    suffix = ".exe" if os.name == "nt" else ""
    llvm = sysroot / "lib/rustlib" / host / "bin"
    profile = output / "coverage.profdata"
    run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *profiles.glob("*.profraw"), "-o", profile], root)
    summary = {}
    # Native process entry, Win32 injection and receiver probes have a separate
    # interactive acceptance boundary. Coverage here cannot qualify that path.
    for module in ("keys", "protocol", "reducer", "worker", "coordinator"):
        source = root / f"crates/desktop-output/src/{module}.rs"
        annotated = run([llvm / f"llvm-cov{suffix}", "show", binaries[0], f"-instr-profile={profile}",
                         "-show-line-counts-or-regions", source], root)
        (output / f"{module}-lines.txt").write_text(annotated, encoding="utf-8")
        lines = source.read_text(encoding="utf-8").splitlines()
        end = next((index + 1 for index, line in enumerate(lines) if line.strip() == "#[cfg(test)]"), len(lines) + 1)
        counts = {int(match[1]): float(match[2].rstrip("kMGT")) > 0 for line in annotated.splitlines()
                  if (match := re.match(r"^\s*(\d+)\|\s*([0-9]+(?:\.[0-9]+)?[kMGT]?)\|", line)) and int(match[1]) < end}
        if not counts:
            raise RuntimeError(f"No instrumented production lines for {module}.")
        covered, total = sum(counts.values()), len(counts)
        summary[module] = dict(covered=covered, lines=total, percent=round(100 * covered / total, 2))
        print(f"{module}: {covered}/{total} ({summary[module]['percent']}%)", flush=True)
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(module["percent"] < 71 for module in summary.values()):
        raise SystemExit("Each portable desktop-output module must meet 71% production line coverage.")


if __name__ == "__main__":
    main()
