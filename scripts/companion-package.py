#!/usr/bin/env python3
"""Build and verify native unsigned Companion development installers. Never publishes or installs."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "apps/companion"
FORMATS = {"windows": "nsis", "linux": "deb", "macos": "app"}
FORBIDDEN = {"osg-cockpit", "osg-controller-input", "osg-virtual-controller", "osg-mapping-host", "osg-mapping", "osg-logic"}
MAX_ARTIFACT = 512 * 1024 * 1024


def run(arguments, cwd=APP, environment=None, capture=False):
    command = [str(value) for value in arguments]
    executable = shutil.which(command[0])
    if executable:
        command[0] = executable
    result = subprocess.run(command, cwd=cwd, env=environment, check=True, text=True,
                            encoding="utf-8", stdout=subprocess.PIPE if capture else None)
    return result.stdout.strip() if capture else None


def native_platform():
    return {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}.get(platform.system())


def validate_target(target, system):
    endings = {"windows": "-pc-windows-msvc", "linux": "-unknown-linux-gnu", "macos": "-apple-darwin"}
    if system not in endings or not target.endswith(endings[system]) or target.split("-")[0] not in {"x86_64", "aarch64"}:
        raise ValueError("Use the native x86_64/aarch64 desktop target; cross-compilation is not this verification path.")


def build_commands(target, system):
    validate_target(target, system)
    manifest = "src-tauri/Cargo.toml"
    return [
        ["cargo", "test", "--locked", "--target", target, "--manifest-path", manifest],
        ["pnpm", "exec", "tauri", "build", "--ci", "--debug", "--no-sign", "--target", target,
         "--bundles", FORMATS[system], "--", "--locked"],
    ]


def dependency_names(metadata):
    """Inspect the actual resolved root graph, excluding unrelated workspace packages."""
    nodes = {item["id"]: item for item in metadata["resolve"]["nodes"]}
    packages = {item["id"]: item["name"] for item in metadata["packages"]}
    pending = [metadata["resolve"]["root"]]
    seen = set()
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        pending.extend(item["pkg"] for item in nodes[current]["deps"])
    names = sorted({packages[item] for item in seen})
    forbidden = FORBIDDEN.intersection(names)
    if forbidden:
        raise ValueError("Companion pulled in host/controller dependencies: " + ", ".join(sorted(forbidden)))
    if "osg-companion" not in names or "osg-companion-service" not in names:
        raise ValueError("Not the standalone Companion dependency graph.")
    return names


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_artifact(path, kind):
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size <= MAX_ARTIFACT:
        raise ValueError("Package must be a bounded regular file.")
    if kind == "nsis":
        with path.open("rb") as handle:
            if path.suffix.lower() != ".exe" or handle.read(2) != b"MZ":
                raise ValueError("Expected an NSIS Windows executable.")
    elif kind == "deb":
        fields = run(["dpkg-deb", "--field", path, "Package", "Version", "Architecture"], capture=True)
        contents = run(["dpkg-deb", "--contents", path], capture=True)
        paths = [line.split()[-1].removeprefix("./") for line in contents.splitlines() if line.split()]
        if "Package: open-sim-gear-companion" not in fields or "usr/bin/osg-companion" not in paths:
            raise ValueError("Debian package is missing the standalone Companion executable.")
        if any(name in contents for name in ["osg-gateway-service", "osg-cockpit", "vJoyInterface.dll"]):
            raise ValueError("Debian package includes an unexpected host/driver executable.")
    elif kind == "app-zip":
        with zipfile.ZipFile(path) as archive:
            items = archive.infolist()
            if len(items) > 8192 or sum(item.file_size for item in items) > MAX_ARTIFACT:
                raise ValueError("macOS bundle exceeds verification budget.")
            if any(Path(item.filename).is_absolute() or ".." in Path(item.filename).parts for item in items):
                raise ValueError("macOS archive contains an escaping path.")
            info = next((item for item in items if item.filename.endswith(".app/Contents/Info.plist")), None)
            binary = next((item for item in items if item.filename.endswith(".app/Contents/MacOS/osg-companion")), None)
            if not info or not binary or info.file_size > 1024 * 1024:
                raise ValueError("macOS archive is missing native app metadata or executable.")
            if plistlib.loads(archive.read(info)).get("CFBundleIdentifier") != "org.opensimgear.companion":
                raise ValueError("Unexpected macOS bundle identity.")
    else:
        raise ValueError("Unknown package format.")
    return {"path": path.name, "format": kind, "bytes": path.stat().st_size, "sha256": sha256(path)}


def verify_manifest(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
        raise ValueError("Invalid development evidence file.")
    evidence = json.loads(path.read_text(encoding="utf-8"))
    if (evidence.get("schemaVersion") != 1 or evidence.get("buildKind") != "development"
            or evidence.get("signingRequested") is not False or evidence.get("published") is not False
            or evidence.get("installed") is not False or not 1 <= len(evidence.get("files", [])) <= 8):
        raise ValueError("Expected explicitly unsigned, uninstalled development evidence.")
    root = path.parent.resolve()
    for item in evidence["files"]:
        artifact = (root / item["path"]).resolve()
        if artifact.parent != root or Path(item["path"]).name != item["path"]:
            raise ValueError("Package path escapes its evidence directory.")
        measured = verify_artifact(artifact, item["format"])
        if measured != item:
            raise ValueError("Package bytes do not match development evidence.")
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / ".research/companion-package")
    parser.add_argument("--target", help="Native Rust target; defaults to rustc's host")
    parser.add_argument("--print-plan", action="store_true")
    parser.add_argument("--verify-only", type=Path, metavar="EVIDENCE_JSON")
    options = parser.parse_args()
    if options.verify_only:
        evidence = verify_manifest(options.verify_only.resolve())
        print(f"Verified {len(evidence['files'])} unsigned development package(s); no install or runtime qualification.")
        return
    system = native_platform()
    if system not in FORMATS:
        raise ValueError("Desktop packaging requires a native Windows, Linux or macOS host.")
    target = options.target or next(line[6:] for line in run(["rustc", "-vV"], capture=True).splitlines() if line.startswith("host: "))
    commands = build_commands(target, system)
    if options.print_plan:
        print(json.dumps({"platform": system, "target": target, "buildKind": "development", "commands": commands}, indent=2))
        return
    output = options.output_dir.resolve() / system
    output.mkdir(parents=True, exist_ok=True)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / ".research/companion-package-target")).resolve()
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target_dir), CI="true")
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path", "src-tauri/Cargo.toml"], capture=True, environment=environment))
    names = dependency_names(metadata)
    for command in commands:
        run(command, environment=environment)
    bundle = target_dir / target / "debug/bundle" / {"windows": "nsis", "linux": "deb", "macos": "macos"}[system]
    config = json.loads((APP / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    pattern = {"windows": f"*_{config['version']}_*-setup.exe", "linux": f"*_{config['version']}_*.deb", "macos": f"{config['productName']}.app"}[system]
    candidates = sorted(bundle.glob(pattern))
    if len(candidates) != 1:
        raise ValueError(f"Expected exactly one native development package in {bundle}, found {len(candidates)}.")
    source = candidates[0]
    if system == "macos":
        artifact = output / (source.name + ".zip")
        run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", source, artifact])
        kind = "app-zip"
    else:
        artifact = output / source.name
        shutil.copy2(source, artifact)
        kind = FORMATS[system]
    evidence = {"schemaVersion": 1, "buildKind": "development", "signingRequested": False,
                "published": False, "installed": False, "platform": system, "target": target,
                "builtAtUtc": datetime.now(timezone.utc).isoformat(), "commands": commands,
                "dependencies": names, "files": [verify_artifact(artifact, kind)]}
    manifest = output / "development-evidence.json"
    manifest.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    verify_manifest(manifest)
    print(f"Verified unsigned development package: {manifest}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
