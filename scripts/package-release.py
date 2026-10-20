#!/usr/bin/env python3
"""Collect component release assets and verify their complete publication set."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BOARDS = {"waveshare-lcd7-s3": ("esp32s3", "0x0"), "sunton-3248s035r": ("esp32", "0x1000")}
SEGMENTS = ("bootloader.bin", "partition-table.bin", "application.bin")


def version_value(value):
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", value):
        raise ValueError("Expected a semantic release version")
    return value


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_commit():
    commit = os.environ.get("GITHUB_SHA") or subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40,64}", commit):
        raise ValueError("Invalid release source commit")
    return commit


def regular(path):
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"Expected a regular artifact: {path}")
    if path.stat().st_size == 0:
        raise ValueError(f"Empty artifact: {path}")
    return path


def exactly_one(paths):
    paths = sorted(paths)
    if len(paths) != 1:
        raise ValueError(f"Expected one build artifact, found {len(paths)}: {paths}")
    return regular(paths[0])


def copy_asset(source, output, name):
    regular(source)
    output.mkdir(parents=True, exist_ok=True)
    target = output / name
    if target.exists() and digest(target) != digest(source):
        raise ValueError(f"Refusing to replace different artifact: {target}")
    if target.resolve() != source.resolve():
        shutil.copyfile(source, target)
    return target


def desktop(platform, version, output):
    prefix = f"cockpit-{version}-{platform}"
    if platform == "windows-x64":
        source = ROOT / ".research/windows-release/artifacts"
        copy_asset(exactly_one(source.glob("*-setup.exe")), output, prefix + "-setup.exe")
        copy_asset(exactly_one(source.glob("*-portable.zip")), output, prefix + "-portable.zip")
    else:
        triple = {"linux-x64": None, "macos-arm64": "aarch64-apple-darwin", "macos-x64": "x86_64-apple-darwin"}[platform]
        source = ROOT / "src-tauri/target"
        if triple:
            source /= triple
        source /= "release/bundle"
        bundles = [("appimage", "*.AppImage", ".AppImage"), ("deb", "*.deb", ".deb")] if platform == "linux-x64" else [("dmg", "*.dmg", ".dmg")]
        for folder, pattern, suffix in bundles:
            copy_asset(exactly_one((source / folder).glob(pattern)), output, prefix + suffix)


def mobile(version, source, output):
    candidates = {
        "mobile-companion-android-arm64-debug.apk": f"mobile-companion-{version}-android-arm64-debug.apk",
        "mobile-companion-android-arm64-release.apk": f"mobile-companion-{version}-android-arm64-release.apk",
        "mobile-companion-ios-arm64-simulator.app.tar.gz": f"mobile-companion-{version}-ios-arm64-simulator.app.tar.gz",
    }
    found = False
    for name, destination in candidates.items():
        if (source / name).exists():
            copy_asset(source / name, output, destination)
            found = True
    if not found:
        raise ValueError("No supported mobile artifacts")


def validate_segments(board, contents):
    """Match the downloader and serial worker's physical image checks."""
    boot = int(BOARDS[board][1], 16)
    offsets = (boot, 0x8000, 0x10000)
    flash_end = 0x400000 if board == "sunton-3248s035r" else 0x800000
    chip_id = 0 if board == "sunton-3248s035r" else 9
    for index, name in enumerate(SEGMENTS):
        data = contents[name]
        limit = offsets[index + 1] if index < 2 else flash_end
        if not data or len(data) > limit - offsets[index]:
            raise ValueError("Board firmware exceeds or is missing from its partition")
        if name != "partition-table.bin" and (len(data) < 24 or data[0] != 0xE9 or int.from_bytes(data[12:14], "little") != chip_id):
            raise ValueError("Firmware image has an invalid header or targets the wrong chip")


def firmware_manifest(board, version, source):
    chip, boot = BOARDS[board]
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("board") != board:
        raise ValueError("Firmware manifest board mismatch")
    records = manifest.get("segments", [])
    if len(records) != 3:
        raise ValueError("Expected exactly three firmware segments")
    for record, name, offset in zip(records, SEGMENTS, (boot, "0x8000", "0x10000")):
        path = regular(source / name)
        if record.get("file") != name or record.get("offset") != offset or record.get("sha256") != digest(path):
            raise ValueError("Firmware layout or digest mismatch")
    validate_segments(board, {name: (source / name).read_bytes() for name in SEGMENTS})
    if manifest.get("version") != version:
        raise ValueError("Firmware manifest version differs from release")
    if manifest.get("sourceCommit") != source_commit():
        raise ValueError("Firmware manifest source differs from release")
    return {"schemaVersion": 1, "component": "esp32-companion", "version": version,
            "board": board, "chip": chip, "sourceCommit": source_commit(), "segments": records}


def firmware(board, version, source, output):
    manifest = firmware_manifest(board, version, source)
    output.mkdir(parents=True, exist_ok=True)
    path = output / f"esp32-companion-{version}-{board}.zip"
    if path.exists():
        raise ValueError("Use a clean board release staging directory")
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(manifest, indent=2) + "\n")
        archive.writestr("README.md", f"# ESP32 companion {version}\n\nBoard: {board}\nChip: {manifest['chip']}\nSource: {manifest['sourceCommit']}\n\n"
                         "Choose the matching board in Cockpit's USB firmware installer.\n"
                         "The manifest records flash offsets and SHA-256 digests for all three images.\n"
                         "Use Cockpit's update flow for an existing companion to preserve its setup.\n")
        for name in SEGMENTS:
            archive.write(source / name, name)
    return path


def finalize(component, version, source, output):
    expected = {
        "cockpit": {f"cockpit-{version}-{suffix}" for suffix in ("windows-x64-setup.exe", "windows-x64-portable.zip", "linux-x64.AppImage", "linux-x64.deb", "macos-arm64.dmg", "macos-x64.dmg")},
        "esp32-companion": {f"esp32-companion-{version}-{board}.zip" for board in BOARDS},
        "mobile-companion": {f"mobile-companion-{version}-ios-arm64-simulator.app.tar.gz"},
    }[component]
    paths = sorted(path for path in source.rglob("*") if path.is_file())
    names = [path.name for path in paths]
    if len(names) != len(set(names)):
        raise ValueError("Duplicate release artifact names")
    actual = set(names)
    if component == "mobile-companion":
        android = {f"mobile-companion-{version}-android-arm64-{kind}.apk" for kind in ("debug", "release")}
        if not actual & android:
            raise ValueError("Missing Android companion APK")
        expected |= actual & android
    if actual != expected:
        raise ValueError(f"Incomplete or unexpected release artifacts; missing={sorted(expected-actual)}, extra={sorted(actual-expected)}")
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError("Use a clean publication staging directory")
    records = []
    for path in paths:
        regular(path)
        if component == "esp32-companion":
            board = next(board for board in BOARDS if path.name == f"esp32-companion-{version}-{board}.zip")
            with zipfile.ZipFile(path) as archive:
                if len(archive.namelist()) != 5 or set(archive.namelist()) != {"manifest.json", "README.md", *SEGMENTS}:
                    raise ValueError("Invalid firmware archive entries")
                manifest = json.loads(archive.read("manifest.json"))
                chip, boot = BOARDS[board]
                if (manifest.get("schemaVersion"), manifest.get("component"), manifest.get("version"), manifest.get("board"), manifest.get("chip"), manifest.get("sourceCommit")) != (1, component, version, board, chip, source_commit()):
                    raise ValueError("Firmware archive provenance mismatch")
                if len(manifest.get("segments", [])) != 3:
                    raise ValueError("Incomplete firmware manifest")
                for record, name, offset in zip(manifest["segments"], SEGMENTS, (boot, "0x8000", "0x10000")):
                    if record.get("file") != name or record.get("offset") != offset or record.get("sha256") != hashlib.sha256(archive.read(name)).hexdigest():
                        raise ValueError("Corrupt firmware archive segment")
                validate_segments(board, {name: archive.read(name) for name in SEGMENTS})
        target = copy_asset(path, output, path.name)
        records.append({"name": target.name, "size": target.stat().st_size, "sha256": digest(target)})
    metadata = {"schemaVersion": 1, "component": component, "version": version, "sourceCommit": source_commit(), "assets": records}
    (output / "release-manifest.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    files = sorted(path for path in output.iterdir() if path.is_file())
    (output / "SHA256SUMS").write_text("".join(f"{digest(path)}  {path.name}\n" for path in files), encoding="ascii")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("desktop", "mobile", "firmware", "finalize"):
        command = commands.add_parser(name)
        command.add_argument("--version", required=True, type=version_value)
        command.add_argument("--output", required=True, type=Path)
        if name != "desktop":
            command.add_argument("--source", required=True, type=Path)
        if name == "desktop":
            command.add_argument("--platform", choices=("windows-x64", "linux-x64", "macos-arm64", "macos-x64"), required=True)
        if name == "firmware":
            command.add_argument("--board", choices=BOARDS, required=True)
        if name == "finalize":
            command.add_argument("--component", choices=("cockpit", "esp32-companion", "mobile-companion"), required=True)
    args = vars(parser.parse_args())
    command = args.pop("command")
    try:
        globals()[command](**args)
    except (ValueError, OSError, KeyError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Release packaging failed: {error}\n")


if __name__ == "__main__":
    main()
