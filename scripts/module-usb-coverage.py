#!/usr/bin/env python3
"""Measure native USB, profile inventory and compiler preview boundaries; require 71% lines per module."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


MODULES = ["profile_game_launch", "profile_session_companion_host", "output_leases", "module_console", "module_usb", "module_usb_fleet", "module_configuration_routes",
           "profile_resources", "profile_resources_game", "profile_session_panels_io", "profile_session_panels_inputs", "profile_session_inspection", "profile_resource_sources", "profile_resource_ports", "profile_compilation", "profile_input_learning", "profile_session_companions", "profile_session_visibility", "profile_session_overrides", "game_activation", "game_activation_runtime", "profile_automatic_store"]
TEST_FILTERS = ["games::launch::tests", "input_learning", "output_leases", "module_", "profile_resource", "profile_compilation", "stm32::tests::events_", "stm32::tests::guarded_restoration", "stm32::tests::atomic_current_claim", "stm32::tests::endpoint_lease", "pico::tests::cached_endpoint_activity", "profile_panels_io", "profile_session_panels_inputs", "profile_session::inspection", "gateways::tests::applied_port_inspection_reads_only_the_exact_native_run_without_loading_a_newer_draft", "gateways::tests::native_profile_and_browser_claims_use_the_same_configuration_gate", "profile_session_companions::tests", "profile_session_host::companion::tests", "profile_session_visibility::tests", "profile_session_overrides::tests", "game_activation", "profile_automatic_store"]


def source_stamp(paths, root):
    return {str(path.relative_to(root)): {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "modifiedNs": path.stat().st_mtime_ns,
    } for path in paths}


def run(args, root, env=None):
    result = subprocess.run([str(arg) for arg in args], cwd=root, env=env,
                            text=True, encoding="utf-8", stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    if result.returncode:
        # Cargo JSON artifacts can dwarf the useful compiler diagnostics.
        for line in result.stdout.splitlines():
            if line.startswith("{"):
                entry = json.loads(line)
                rendered = entry.get("message", {}).get("rendered")
                if rendered:
                    print(rendered, file=sys.stderr, end="")
            else:
                print(line, file=sys.stderr)
        result.check_returncode()
    return result.stdout


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=root / ".research/module-usb-coverage")
    parser.add_argument("--module", action="append", choices=MODULES,
                        help="Focused local measurement only; CI's default still gates every module.")
    parser.add_argument("--test-filter", action="append",
                        help="Focused local test filter; requires --module. CI runs the complete filter list.")
    args = parser.parse_args()
    if args.test_filter and (not args.module or any(not value.strip() for value in args.test_filter)):
        parser.error("--test-filter requires --module and nonempty filter values")
    measured = args.module or MODULES
    filters = args.test_filter or TEST_FILTERS
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    # Never leave a previous successful summary behind after a failed rerun.
    for name in ["summary.json", "evidence.json", "tests.log"]:
        (output / name).unlink(missing_ok=True)
    tracked = [root / f"src-tauri/src/{module}{suffix}.rs"
               for module in MODULES for suffix in ["", "_tests"]]
    tracked.extend([root / "src-tauri/src/profile_session_host.rs", root / "src-tauri/src/companion_host.rs", root / "src-tauri/src/stm32.rs", root / "src-tauri/src/pico.rs", root / "src-tauri/src/games.rs", root / "src-tauri/src/game_discovery.rs", root / "src-tauri/src/game_context.rs", root / "src-tauri/src/game_context_runtime.rs", root / "src-tauri/src/profile_session.rs", root / "src-tauri/src/profile_session_stage.rs", root / "src-tauri/src/profile_session_journal.rs", root / "src-tauri/Cargo.toml", root / "src-tauri/Cargo.lock"])
    if not args.module:
        # The full gate also pins Manager construction/admission. A focused
        # production-module report is explicitly not whole-Manager evidence.
        tracked.append(root / "src-tauri/src/gateways.rs")
    # Preview semantics depend on the exact portable compiler and signal engine.
    # Record those sources/tests too, so changes during this measurement cannot
    # leave apparently current native-preview evidence behind.
    for crate in ["mapping", "profiles"]:
        crate_root = root / "crates" / crate
        tracked.extend(sorted((crate_root / "src").glob("*.rs")))
        tracked.extend([crate_root / "Cargo.toml", crate_root / "Cargo.lock"])
    initial_stamp = source_stamp(tracked, root)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    env = dict(os.environ,
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(output / "target")),
               RUSTFLAGS="-C instrument-coverage",
               LLVM_PROFILE_FILE=str(profiles / "build-%m-%p.profraw"))
    artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json",
                     "--manifest-path", root / "src-tauri/Cargo.toml"], root, env)
    binaries = []
    for line in artifacts.splitlines():
        if not line.startswith("{"):
            continue
        entry = json.loads(line)
        if (entry.get("reason") == "compiler-artifact" and entry.get("executable")
                and entry.get("profile", {}).get("test")):
            binaries.append(entry["executable"])
    if len(binaries) != 1:
        raise RuntimeError("Expected one instrumented Cockpit library test executable.")
    test_env = dict(env, LLVM_PROFILE_FILE=str(profiles / "test-%m-%p.profraw"))
    logs = []
    for test_filter in filters:
        tests = subprocess.run([binaries[0], test_filter, "--test-threads=1"],
                               cwd=root, env=test_env, text=True, encoding="utf-8",
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        logs.append(tests.stdout)
        (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
        if tests.returncode:
            print(tests.stdout, file=sys.stderr, end="")
            tests.check_returncode()
    evidence = list(profiles.glob("test-*.profraw"))
    if len(evidence) < len(filters):
        raise RuntimeError("Successful USB test processes produced incomplete coverage evidence.")
    final_stamp = source_stamp(tracked, root)
    if initial_stamp != final_stamp:
        changed = [name for name in initial_stamp if initial_stamp[name] != final_stamp[name]]
        raise RuntimeError(f"Sources or tests changed during coverage; rerun on the final files: {changed}")
    sysroot = Path(run(["rustc", "--print", "sysroot"], root).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"], root), re.M).group(1)
    suffix = ".exe" if os.name == "nt" else ""
    llvm = sysroot / "lib/rustlib" / host / "bin"
    profile = output / "coverage.profdata"
    run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *evidence, "-o", profile], root)
    summary = {}
    for module in measured:
        source = root / f"src-tauri/src/{module}.rs"
        annotated = run([llvm / f"llvm-cov{suffix}", "show", binaries[0],
                         f"-instr-profile={profile}", "-show-line-counts-or-regions", source], root)
        (output / f"{module}-lines.txt").write_text(annotated, encoding="utf-8")
        lines = source.read_text(encoding="utf-8").splitlines()
        end = next((i + 1 for i, line in enumerate(lines) if line == "#[cfg(test)]"), len(lines) + 1)
        counts = {}
        for line in annotated.splitlines():
            match = re.match(r"^\s*(\d+)\|\s*([^|]+)\|", line)
            if not match or int(match[1]) >= end:
                continue
            count = match[2].strip()
            if re.fullmatch(r"[0-9]+(?:\.[0-9]+)?[kMGT]?", count):
                counts[int(match[1])] = float(count.rstrip("kMGT")) > 0
        if not counts:
            raise RuntimeError(f"No production coverage recorded for {module}.")
        covered = sum(counts.values())
        percent = 100 * covered / len(counts)
        summary[module] = dict(covered=covered, lines=len(counts), percent=round(percent, 2))
        print(f"{module}: {covered}/{len(counts)} production lines ({percent:.2f}%)", flush=True)
    (output / "evidence.json").write_text(json.dumps({"sources": final_stamp, "testFilters": filters,
        "measuredModules": measured, "focused": bool(args.module), "rustc": run(["rustc", "-vV"], root)}, indent=2) + "\n", encoding="utf-8")
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(item["percent"] < 71 for item in summary.values()):
        raise SystemExit("Every native USB/inventory/compiler module requires at least 71% production line coverage.")


if __name__ == "__main__":
    main()
