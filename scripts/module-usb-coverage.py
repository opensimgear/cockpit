#!/usr/bin/env python3
"""Measure native USB, profile inventory and compiler preview boundaries; require 71% lines per module."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


MODULES = ["profile_companion_retention", "profile_retired_recovery", "profile_game_orchestration_runtime", "profile_game_orchestration", "profile_session_launch_host", "profile_session_action_host", "profile_session_topology", "profile_session_launch", "profile_session_actions", "profile_session_visibility_host", "profile_companion_authoring", "profile_session_action_lease", "profile_automatic", "profile_game_launch", "profile_session_companion_host", "output_leases", "module_console", "module_usb", "module_usb_fleet", "module_configuration_routes",
           "profile_resources", "profile_resources_game", "profile_session_panels_io", "profile_session_panels_inputs", "profile_session_inspection", "profile_resource_sources", "profile_resource_ports", "profile_compilation", "profile_input_learning", "profile_session_companions", "profile_session_visibility", "profile_session_overrides", "game_activation", "game_activation_runtime", "profile_automatic_store"]
TEST_FILTERS = ["gateways::companion_retention::tests", "profile_retired_recovery::tests", "gateways::automatic::launch::runtime::tests", "gateways::automatic::launch::tests", "profile_session_host::launch::tests", "profile_session_host::action::tests", "profile_session::topology", "profile_session::launch", "profile_session_actions::tests", "profile_session_host::visibility::tests", "companion_host::authoring", "gateways::companion_authoring", "profile_session_action_lease::tests", "gateways::automatic::tests", "games::launch::tests", "input_learning", "output_leases", "module_", "profile_resource", "profile_compilation", "stm32::tests::events_", "stm32::tests::guarded_restoration", "stm32::tests::atomic_current_claim", "stm32::tests::endpoint_lease", "pico::tests::cached_endpoint_activity", "profile_panels_io", "profile_session_panels_inputs", "profile_session::inspection", "gateways::tests::applied_port_inspection_reads_only_the_exact_native_run_without_loading_a_newer_draft", "gateways::tests::native_profile_and_browser_claims_use_the_same_configuration_gate", "profile_session_companions::tests", "profile_session_host::companion::tests", "profile_session_visibility::tests", "profile_session_overrides::tests", "game_activation", "profile_automatic_store"]


MODULES.append("stm32_publication")
TEST_FILTERS.append("stm32::publication::tests")
# The overrides module is a declaration-only facade; gate its actual native
# ownership actor and Coordinator integration, with their real shared fixtures.
MODULES.remove("profile_session_overrides")
MODULES.extend(["profile_publication", "profile_session_publication_host", "profile_session_virtual", "profile_recovery_transport"])
TEST_FILTERS.extend(["profile_session_virtual::tests", "profile_session_host::output_handoff_tests",
                     "gateways::recovery_transport::tests", "stm32::tests::recovery_transport"])
TEST_FILTERS.append("profile_session_overrides::publication")
MODULE_TEST_SOURCES = {
    "window_persistence": [],
    "companion/storage/panels": [],
    "system_inventory": [],
    "system_inventory_associations": [],
    "profile_recovery_transport": ["profile_recovery_transport_tests", "profile_recovery_transport_stm32_tests"],
    "profile_session_virtual": ["profile_session_output_handoff_tests"],
    "profile_publication": ["profile_publication_tests", "profile_publication_rebind_tests",
                            "profile_publication_action_tests"],
    "profile_session_publication_host": ["profile_publication_tests",
                                         "profile_publication_action_tests"],
}
MODULES.extend(["system_inventory", "system_inventory_associations"])
TEST_FILTERS.append("system_inventory")
MODULES.append("companion/storage/panels")
TEST_FILTERS.append("companion_panels::tests")
MODULES.append("window_persistence")
TEST_FILTERS.append("window_persistence::tests")


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


def run_test_filters(binary, filters, root, env, profiles, output, timeout_seconds=300):
    evidence = []
    invocations = []
    logs = []
    for index, test_filter in enumerate(filters):
        # Windows can reuse a PID during this sequential suite. LLVM's %m-%p
        # basename then repeats, so global file counts cannot prove that each
        # invocation wrote evidence. Give every invocation its own empty scope.
        directory = profiles / f"test-{index:03d}"
        directory.mkdir()
        test_env = dict(env, LLVM_PROFILE_FILE=str(directory / "test-%m-%p.profraw"))
        logs.append(f"\n=== {test_filter} ===\n")
        (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
        print(f"Running native coverage filter: {test_filter}", flush=True)
        try:
            tests = subprocess.run([binary, test_filter, "--test-threads=1"],
                                   cwd=root, env=test_env, text=True, encoding="utf-8",
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   timeout=timeout_seconds)
        except subprocess.TimeoutExpired as error:
            partial = error.stdout or ""
            if isinstance(partial, bytes):
                partial = partial.decode("utf-8", errors="replace")
            message = f"Native coverage filter {test_filter!r} timed out after {timeout_seconds:g}s."
            logs.extend([partial, message])
            (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
            print(partial, file=sys.stderr, end="")
            raise RuntimeError(message) from error
        logs.append(tests.stdout)
        (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
        if tests.returncode:
            print(tests.stdout, file=sys.stderr, end="")
            tests.check_returncode()
        files = sorted(directory.glob("test-*.profraw"))
        if not files or any(not path.is_file() or path.is_symlink() or path.stat().st_size == 0
                            for path in files):
            raise RuntimeError(
                f"Successful USB test filter {test_filter!r} produced missing or empty "
                f"coverage evidence in {directory}.")
        evidence.extend(files)
        invocations.append({"testFilter": test_filter, "profiles": [
            {"path": str(path.relative_to(output)), "bytes": path.stat().st_size}
            for path in files]})
    return evidence, invocations


def tracked_source_paths(root, measured, focused):
    tracked = [root / f"src-tauri/src/{name}.rs"
               for module in MODULES
               for name in [module, *MODULE_TEST_SOURCES.get(module, [f"{module}_tests"])]]
    tracked.extend(root / f"src-tauri/src/{name}.rs" for name in
                   ["profile_session_overrides", "profile_session_overrides_tests"])
    tracked.extend([root / "src-tauri/src/lib.rs", root / "src-tauri/src/profile_session_host.rs", root / "src-tauri/src/companion/host/mod.rs", root / "src-tauri/src/companion/host/commands.rs", root / "src-tauri/src/companion/host/presentation.rs", root / "src-tauri/src/companion/storage/host_settings.rs", root / "src-tauri/src/stm32.rs", root / "src-tauri/src/pico.rs", root / "src-tauri/src/games.rs", root / "src-tauri/src/game_discovery.rs", root / "src-tauri/src/game_context.rs", root / "src-tauri/src/game_context_runtime.rs", root / "src-tauri/src/profile_session.rs", root / "src-tauri/src/profile_session_stage.rs", root / "src-tauri/src/profile_session_journal.rs", root / "src-tauri/Cargo.toml", root / "src-tauri/Cargo.lock"])
    tracked.extend([Path(__file__).resolve(), root / "scripts/test_module_usb_coverage.py"])
    if not focused or "profile_automatic" in measured or "profile_game_orchestration_runtime" in measured or "profile_companion_retention" in measured or "profile_recovery_transport" in measured:
        # The full gate also pins Manager construction/admission. A focused
        # production-module report is explicitly not whole-Manager evidence.
        tracked.append(root / "src-tauri/src/gateways.rs")
    # Preview semantics depend on the exact portable compiler and signal engine.
    # Record those sources/tests too, so changes during this measurement cannot
    # leave apparently current native-preview evidence behind.
    for crate in ["mapping", "profiles", "virtual-controller"]:
        crate_root = root / "crates" / crate
        tracked.extend(sorted((crate_root / "src").glob("*.rs")))
        tracked.extend([crate_root / "Cargo.toml", crate_root / "Cargo.lock"])
    # Passive companion authoring depends on metadata-only Hub projection and
    # native paired-identity validation, never operator status/session expiry.
    tracked.extend([root / "src-tauri/src/profile_companion_authoring_gateway.rs",
                    root / "crates/companion-service/src/hub.rs",
                    root / "crates/companion-service/src/profile.rs",
                    root / "crates/companion-service/src/publication.rs",
                    root / "crates/mapping-host/src/companions.rs"])
    # Profile hide handoff depends on the exact native owner/journal proof,
    # including restore-only worker admission rather than current hidden state.
    tracked.append(root / "crates/controller-input/src/lib.rs")
    tracked.append(root / "crates/controller-input/src/inventory.rs")
    tracked.extend(root / f"crates/controller-input/src/visibility/{name}.rs" for name in
                   ["mod", "game", "game_tests", "game_handoff_tests", "handoff", "handoff_tests",
                    "journal", "policy", "runtime", "service", "service_tests"])
    return tracked


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
    tracked = tracked_source_paths(root, measured, bool(args.module))
    initial_stamp = source_stamp(tracked, root)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    env = dict(os.environ,
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(output / "target")),
               RUSTFLAGS="-C instrument-coverage",
               LLVM_PROFILE_FILE=str(profiles / "build-%m-%p.profraw"))
    print("Building instrumented native coverage tests", flush=True)
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
    binary = profiles / Path(binaries[0]).name
    shutil.copy2(binaries[0], binary)
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    evidence, invocations = run_test_filters(binary, filters, root, env, profiles, output)
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
        annotated = run([llvm / f"llvm-cov{suffix}", "show", binary,
                         f"-instr-profile={profile}", "-show-line-counts-or-regions", source], root)
        (output / f"{module.replace(chr(47), chr(45))}-lines.txt").write_text(annotated, encoding="utf-8")
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
        "testInvocations": invocations,
        "executable": str(binary), "executableSha256": binary_hash, "cargoArtifact": binaries[0],
        "measuredModules": measured, "focused": bool(args.module), "rustc": run(["rustc", "-vV"], root)}, indent=2) + "\n", encoding="utf-8")
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(item["percent"] < 71 for item in summary.values()):
        raise SystemExit("Every native USB/inventory/compiler module requires at least 71% production line coverage.")


if __name__ == "__main__":
    main()
