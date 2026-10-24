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
    parser.add_argument("--desktop-only", action="store_true", help="Measure only native desktop operators")
    parser.add_argument("--companion-tools-only", action="store_true", help="Measure typed MCP companion tools with their native integration tests")
    parser.add_argument("--test-threads", type=int, choices=range(1,33), help="Bound native regression concurrency")
    options = parser.parse_args()
    if options.desktop_only and options.service_only:
        parser.error("Choose either desktop-only or service-only")
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
    targets = [("service", root / "crates/companion-service/Cargo.toml", "", [root / f"crates/companion-service/src/{name}.rs" for name in ["lib", "sources", "persistence", "publication", "feedback", "hub", "hub_display", "profile", "profile_owned", "hub_feedback", "hub_assignments", "tls", "server", "client"]])]
    targets[0][3].append(root / "crates/companion-service/src/persistence/sqlite.rs")
    targets[0][3].extend(root / f"crates/companion-service/src/{name}.rs" for name in ["client_cache/mod", "client/cache_validation", "host_content/mod", "host_content/selection", "persistence/sqlite_host"])
    targets[0][3].extend(root / f"crates/companion-service/src/publication/{name}.rs"
                         for name in ["compiled", "compiled/artifact", "compiled/behavior", "widget_source", "widget_design", "widget_design/helpers"])
    targets.append(("serial", root / "crates/companion-serial/Cargo.toml", "", [root / "crates/companion-serial/src/boot_wait.rs"]))
    if not options.service_only:
        # Profile publication, authoring and lifecycle tests call this host from
        # other modules. Measure those real integrations as well as its unit tests.
        targets.append(("desktop", root / "src-tauri/Cargo.toml", "", [root / "src-tauri/src/companion/host/mod.rs", root / "src-tauri/src/companion/host/commands.rs", root / "src-tauri/src/companion/host/presentation.rs", root / "src-tauri/src/companion/storage/host_settings.rs", root / "src-tauri/src/mcp_tools/host_settings.rs", root / "src-tauri/src/companion/storage/panels.rs", root / "src-tauri/src/local_firmware.rs", root / "src-tauri/src/companion/storage/widgets.rs", root / "src-tauri/src/companion/storage/workspace.rs", root / "src-tauri/src/companion/storage/workspace_validation.rs", root / "src-tauri/src/mcp_tools/workspace.rs", root / "src-tauri/src/mcp_tools/companion_controls.rs", root / "src-tauri/src/mcp_tools/companion_catalogue.rs"]))
    if options.desktop_only:
        targets = [target for target in targets if target[0] == "desktop"]
    if options.companion_tools_only:
        targets = [("desktop", root / "src-tauri/Cargo.toml", "mcp_tools::companion_controls", [root / f"src-tauri/src/mcp_tools/{name}.rs" for name in ["companion_controls", "companion_catalogue"]])]
    summary = {}
    for name, manifest, selection, sources in targets:
        artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json", "--manifest-path", manifest], root, environment)
        binaries = [item["executable"] for line in artifacts.splitlines()
                    if (item := json.loads(line)).get("reason") == "compiler-artifact"
                    and item.get("executable") and item.get("profile", {}).get("test")]
        if len(binaries) != 1:
            raise RuntimeError(f"Expected one {name} library test executable.")
        test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / f"{name}-%m-%p.profraw"))
        arguments = [binaries[0], *([selection] if selection else []),
                     *([f"--test-threads={options.test_threads}"] if options.test_threads else [])]
        log_path = output / f"{name}-tests.log"
        with log_path.open("w", encoding="utf-8") as log:
            tests = subprocess.Popen(arguments, cwd=root, env=test_environment, text=True, encoding="utf-8",
                                     stdout=log, stderr=subprocess.STDOUT)
            tests.wait()
        if tests.returncode:
            print(log_path.read_text(encoding="utf-8"), flush=True)
            raise subprocess.CalledProcessError(tests.returncode, arguments)
        raw_profiles = list(profiles.glob(f"{name}-*-{tests.pid}.profraw"))
        if not raw_profiles:
            raise RuntimeError(f"The {name} process produced no coverage evidence.")
        merged = output / f"{name}.profdata"
        run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *raw_profiles, "-o", merged], root)
        for source in sources:
            annotation = run([llvm / f"llvm-cov{suffix}", "show", binaries[0], f"-instr-profile={merged}", "-show-line-counts-or-regions", source], root)
            relative = source.relative_to(root).as_posix()
            annotation_name = relative.replace("/", "-")
            (output / f"{name}-{annotation_name}-lines.txt").write_text(annotation, encoding="utf-8")
            lines = source.read_text(encoding="utf-8").splitlines()
            # lib.rs declares test modules but has production exports/functions afterward.
            end = len(lines) + 1 if source.name == "lib.rs" else next((index + 1 for index, line in enumerate(lines) if line == "#[cfg(test)]"), len(lines) + 1)
            # LLVM abbreviates hot counts (for example 1.28k). Include those
            # production lines in both numerator and denominator as well.
            counts = {int(match[1]): float(match[2].rstrip("kMGT")) > 0 for line in annotation.splitlines()
                      if (match := re.match(r"^\s*(\d+)\|\s*([\d.]+[kMGT]?)\|", line)) and int(match[1]) < end}
            if not counts:
                raise RuntimeError(f"No instrumented production lines for {source}.")
            covered = sum(counts.values())
            percent = 100 * covered / len(counts)
            key = f"{name}/{relative}"
            summary[key] = {"covered": covered, "lines": len(counts), "percent": round(percent, 2)}
            print(f"{key}: {covered}/{len(counts)} production lines ({percent:.2f}%)", flush=True)
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(item["percent"] < 71 for item in summary.values()):
        raise SystemExit("Each companion production module must meet 71% line coverage.")


if __name__ == "__main__":
    main()
