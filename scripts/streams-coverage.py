"""Run portable stream conformance tests and require 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(arguments, root, env=None):
    return subprocess.check_output([str(value) for value in arguments], cwd=root,
                                   env=env, text=True, encoding="utf-8")


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=root / ".research/streams-coverage")
    output = parser.parse_args().output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / "target"),
               RUSTFLAGS="-C instrument-coverage",
               LLVM_PROFILE_FILE=str(profiles / "%m-%p.profraw"))
    artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json",
                     "--manifest-path", root / "crates/streams/Cargo.toml"], root, env)
    binaries = [item["executable"] for line in artifacts.splitlines()
                if (item := json.loads(line)).get("reason") == "compiler-artifact"
                and item.get("executable") and item.get("profile", {}).get("test")]
    if len(binaries) != 1:
        raise RuntimeError("Expected one instrumented stream library test executable")
    log = run([binaries[0]], root, env)
    (output / "tests.log").write_text(log, encoding="utf-8")
    print(log, flush=True)
    sysroot = Path(run(["rustc", "--print", "sysroot"], root).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"], root), re.M).group(1)
    suffix = ".exe" if os.name == "nt" else ""
    llvm = sysroot / "lib/rustlib" / host / "bin"
    profile = output / "coverage.profdata"
    run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *profiles.glob("*.profraw"),
         "-o", profile], root)
    summary = {}
    for source in sorted((root / "crates/streams/src").glob("*.rs")):
        if source.stem == "tests" or source.stem.endswith("_tests"):
            continue
        annotated = run([llvm / f"llvm-cov{suffix}", "show", binaries[0],
                         f"-instr-profile={profile}", "-show-line-counts-or-regions", source], root)
        (output / f"{source.stem}-lines.txt").write_text(annotated, encoding="utf-8")
        counts = {int(match[1]): float(match[2]) * {"": 1, "k": 1000, "M": 1000000}[match[3]]
                  for line in annotated.splitlines()
                  if (match := re.match(r"^\s*(\d+)\|\s*(\d+(?:\.\d+)?)([kM]?)\|", line))}
        if not counts:
            if re.search(r"\bfn\s+\w+", source.read_text(encoding="utf-8")):
                raise RuntimeError(f"No production coverage for {source.name}")
            continue  # Type declarations/module exports have no executable lines.
        covered = sum(count > 0 for count in counts.values())
        percent = 100 * covered / len(counts)
        summary[source.name] = dict(covered=covered, lines=len(counts), percent=round(percent, 2))
        print(f"{source.name}: {covered}/{len(counts)} production lines ({percent:.2f}%)", flush=True)
    if not summary:
        raise RuntimeError("No instrumented stream production modules")
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(item["percent"] < 71 for item in summary.values()):
        raise SystemExit("Each stream module requires 71% production line coverage")


if __name__ == "__main__":
    main()
