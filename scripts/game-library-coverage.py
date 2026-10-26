#!/usr/bin/env python3
"""Require 71% production line coverage for installed-library discovery and MCP."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from native_coverage_sources import measure_module, source_stamp

ROOT = Path(__file__).resolve().parents[1]
MODULES = [f"games/library/{name}.rs" for name in ["mod", "steam", "epic", "battlenet", "battlenet_artwork", "registry", "xbox", "disk", "platforms", "executable_ranking", "artwork", "artwork_choices"]] + [
    "games/artwork_options.rs", "games/artwork/mod.rs",
    "games/review/mod.rs", "games/review/reconcile.rs", "games/review/worker.rs", "games/review/completion.rs",
    "games/storage/installations.rs", "games/tags/mod.rs", "game_metadata_api.rs", "game_discovery.rs", "games.rs",
    "game_context_runtime.rs", "games/integration/mod.rs", "games/integration/requests.rs",
    "games/integration/inspection.rs", "games/integration/provider_settings.rs",
    "mcp_tools/game_library.rs", "profile_local_settings.rs"]
TEST_FILTERS = ["game_library", "game_artwork", "game_discovery", "games::discovery_review", "games::metadata_api",
                "game_installations", "local_settings", "games::tests", "game_context", "games::context_runtime",
                "game_integration_setup", "discovery_apply", "game_get_state", "mcp_tools::game_library::tests"]


def run(args, env=None):
    result = subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env,
                            text=True, encoding="utf-8", errors="replace",
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        print(result.stdout, flush=True)
        result.check_returncode()
    return result.stdout


def main():
    output = ROOT / ".research/game-library-coverage"
    output.mkdir(parents=True, exist_ok=True)
    for name in ["summary.json", "source-hashes.json", "tests.log"]:
        (output / name).unlink(missing_ok=True)
    sources = [ROOT / "src-tauri/src" / name for name in MODULES]
    tracked = sources + list((ROOT / "src-tauri/src").rglob("*.rs")) + [
        Path(__file__).resolve(), ROOT / "scripts/native_coverage_sources.py",
        ROOT / "scripts/test_native_coverage_sources.py", ROOT / "scripts/desktop-output-coverage.py", ROOT / "src-tauri/Cargo.toml",
        ROOT / "src-tauri/Cargo.lock"]
    source_hashes = source_stamp(tracked, ROOT)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    suffix = ".exe" if os.name == "nt" else ""
    sysroot = Path(run(["rustc", "--print", "sysroot"]).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"]), re.M)[1]
    llvm = sysroot / "lib/rustlib" / host / "bin"
    env = dict(os.environ, RUSTFLAGS="-C instrument-coverage",
               CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(ROOT / ".research/module-usb-coverage/target")),
               LLVM_PROFILE_FILE=str(profiles / "build-%m-%p.profraw"))
    print("Building instrumented native library tests", flush=True)
    built = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json",
                 "--manifest-path", "src-tauri/Cargo.toml"], env)
    binaries = [entry["executable"] for line in built.splitlines() if line.startswith("{")
                and (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry.get("executable") and entry.get("profile", {}).get("test")]
    if len(binaries) != 1:
        raise RuntimeError("Expected exactly one native test binary")
    binary = binaries[0]
    logs = []
    for index, query in enumerate(TEST_FILTERS):
        print(f"Running {query}", flush=True)
        test_env = dict(env, LLVM_PROFILE_FILE=str(profiles / f"tests-{index}-%m-%p.profraw"))
        log = run([binary, query, "--test-threads=1"], test_env)
        logs.append(log)
        (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
        if not re.search(r"test result: ok\. [1-9]\d* passed;", log):
            raise RuntimeError(f"Native coverage filter {query!r} did not run matching tests")
        evidence = list(profiles.glob(f"tests-{index}-*.profraw"))
        if not evidence or any(not path.is_file() or path.stat().st_size == 0 for path in evidence):
            raise RuntimeError(f"Native coverage filter {query!r} did not write nonempty coverage profiles")
    (output / "tests.log").write_text("\n".join(logs), encoding="utf-8")
    if source_hashes != source_stamp(tracked, ROOT):
        raise RuntimeError("Launcher sources changed during coverage; rebuild and rerun")
    (output / "source-hashes.json").write_text(json.dumps(source_hashes, indent=2) + "\n", encoding="utf-8")
    profile = output / "coverage.profdata"
    raw = list(profiles.glob("tests-*.profraw"))
    if not raw:
        raise RuntimeError("Tests did not write native coverage profiles")
    run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *raw, "-o", profile])
    summary = {}
    for name in MODULES:
        source = ROOT / "src-tauri/src" / name
        result, annotated = measure_module(source, lambda path: run([
            llvm / f"llvm-cov{suffix}", "show", binary, f"-instr-profile={profile}",
            "-show-line-counts-or-regions", path]), ROOT)
        (output / f"{name.replace('/', '-')}-lines.txt").write_text(annotated, encoding="utf-8")
        summary[name] = result
        print(f"{name}: {result['covered']}/{result['lines']} ({result['percent']}%)", flush=True)
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(row["covered"] * 100 < row["lines"] * 71 for row in summary.values()):
        raise SystemExit("Installed-library production coverage must be at least 71% per module")


if __name__ == "__main__":
    main()
