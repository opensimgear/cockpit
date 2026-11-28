"""Run H1 unit and integration tests and gate each portable production module."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from native_coverage_sources import production_code, production_counts


ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates/hid-report"


def output(args, env=None):
    return subprocess.check_output(args, cwd=ROOT, env=env, text=True, encoding="utf-8")


def main():
    build = ROOT / ".research/hid-report-coverage"
    build.mkdir(parents=True, exist_ok=True)
    sysroot = Path(output(["rustc", "--print", "sysroot"]).strip())
    host = next(line.split(": ", 1)[1] for line in output(["rustc", "-vV"]).splitlines()
                if line.startswith("host: "))
    llvm = sysroot / "lib/rustlib" / host / "bin"
    suffix = ".exe" if os.name == "nt" else ""
    with tempfile.TemporaryDirectory(dir=build) as tmp:
        env = dict(os.environ, CARGO_TARGET_DIR=str(build / "target"),
                   RUSTFLAGS="-C instrument-coverage", LLVM_PROFILE_FILE=f"{tmp}/%m-%p.profraw")
        args = ["cargo", "test", "--locked", "--lib", "--tests",
                "--manifest-path", str(CRATE / "Cargo.toml")]
        artifacts = output(args + ["--no-run", "--message-format=json"], env)
        binaries = [record["executable"] for record in map(json.loads, artifacts.splitlines())
                    if record.get("reason") == "compiler-artifact" and record.get("executable")
                    and record.get("profile", {}).get("test")]
        if not binaries:
            raise RuntimeError("Cargo produced no instrumented test executables")
        for binary in binaries:
            subprocess.run([binary], cwd=CRATE, env=env, check=True)
        profile = Path(tmp) / "merged.profdata"
        output([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *Path(tmp).glob("*.profraw"),
                "-o", profile])
        command = [llvm / f"llvm-cov{suffix}", "show", binaries[0], f"-instr-profile={profile}"]
        for binary in binaries[1:]:
            command.extend(["-object", binary])
        failed = []
        for path in sorted((CRATE / "src").rglob("*.rs")):
            if path.stem == "tests" or path.stem.endswith("_tests") or "tests" in path.parts:
                continue
            source = path.read_text(encoding="utf-8")
            counts = production_counts(source, output(command + [path]))
            if not counts and "fn " not in production_code(source):
                continue
            covered = sum(value > 0 for value in counts.values())
            percent = 100 * covered / len(counts) if counts else 0
            print(f"{path.relative_to(CRATE)}: {covered}/{len(counts)} lines ({percent:.2f}%)")
            if percent < 71:
                failed.append(path.name)
        if failed:
            raise SystemExit(f"Production coverage below 71%: {', '.join(failed)}")


if __name__ == "__main__":
    main()
