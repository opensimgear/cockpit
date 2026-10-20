#!/usr/bin/env python3
"""Publish a complete component asset set; never replace a published release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import uuid


COMPONENTS = ("cockpit", "esp32-companion", "mobile-companion")


def gh(*arguments, check=True):
    return subprocess.run(["gh", *arguments], check=check, capture_output=True, text=True)


def release(tag):
    result = gh("release", "view", tag, "--json", "isDraft,body,url,name,assets", check=False)
    if result.returncode:
        # A missing release is the only lookup failure that permits creation.
        message = result.stderr.lower()
        if "release not found" in message or "http 404" in message:
            return None
        raise RuntimeError("Cannot inspect GitHub release: " + result.stderr.strip())
    return json.loads(result.stdout)


def verify_assets(directory, expected_names=None, identity=None):
    files = {path.name: path for path in directory.iterdir() if path.is_file()}
    if any(path.is_symlink() or not path.is_file() for path in directory.iterdir()):
        raise ValueError("Release assets must be regular files in one directory")
    if expected_names is not None and set(files) != set(expected_names):
        raise ValueError("GitHub release asset names differ from the complete expected set")
    if not {"SHA256SUMS", "release-manifest.json"}.issubset(files):
        raise ValueError("Release assets require SHA256SUMS and release-manifest.json")
    checksums = {}
    for line in files["SHA256SUMS"].read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)", line)
        if not match or match[2] in checksums:
            raise ValueError("Invalid or duplicate release checksum entry")
        checksums[match[2]] = match[1]
    if set(checksums) != set(files) - {"SHA256SUMS"}:
        raise ValueError("Release checksums must cover every asset except SHA256SUMS")
    for name, digest in checksums.items():
        if hashlib.sha256(files[name].read_bytes()).hexdigest() != digest:
            raise ValueError("Release asset checksum mismatch: " + name)
    manifest = json.loads(files["release-manifest.json"].read_text(encoding="utf-8"))
    if manifest.get("schemaVersion") != 1:
        raise ValueError("Unsupported release manifest schema")
    if identity and any(manifest.get(key) != value for key, value in identity.items()):
        raise ValueError("Release manifest component, version or source commit differs from the tag")
    records = manifest.get("assets", [])
    if (len(records) != len(files) - 2
            or {record.get("name") for record in records} != set(files) - {"SHA256SUMS", "release-manifest.json"}):
        raise ValueError("Release manifest must describe every payload asset exactly once")
    for record in records:
        name = record["name"]
        if record.get("sha256") != checksums[name] or record.get("size") != files[name].stat().st_size:
            raise ValueError("Release manifest payload metadata mismatch: " + name)
    return files


def verify_remote(tag, metadata, local_files, notes, identity, compare_local):
    names = [asset["name"] for asset in metadata["assets"]]
    if len(names) != len(set(names)) or set(names) != set(local_files):
        raise ValueError("GitHub release does not contain the complete expected asset set")
    if metadata["body"].replace("\r\n", "\n").rstrip("\n") != notes.rstrip("\n"):
        raise ValueError("GitHub release notes differ from the committed release notes")
    with tempfile.TemporaryDirectory(prefix="osg-release-verify-") as temporary:
        destination = Path(temporary)
        gh("release", "download", tag, "--dir", str(destination))
        downloaded = verify_assets(destination, local_files, identity)
        if compare_local:
            for name, path in downloaded.items():
                if path.read_bytes() != local_files[name].read_bytes():
                    raise ValueError("Uploaded release asset differs from the verified build: " + name)


def publish(tag, component, version, notes_file, assets):
    if component not in COMPONENTS or tag != f"{component}-v{version}":
        raise ValueError("Component, version and tag do not match")
    source = subprocess.run(["git", "rev-parse", "HEAD"], check=True,
                            capture_output=True, text=True).stdout.strip()
    if os.environ.get("GITHUB_SHA", source) != source:
        raise ValueError("Checked-out source differs from the tag event commit")
    identity = dict(component=component, version=version, sourceCommit=source)
    files = verify_assets(assets, identity=identity)
    notes = notes_file.read_text(encoding="utf-8").replace("\r\n", "\n")
    if not notes.strip():
        raise ValueError("Committed release notes are empty")
    current = release(tag)
    if current and not current["isDraft"]:
        # A rebuild can produce different bytes. Verify the original immutable set
        # against its own checksums and committed notes, then leave it untouched.
        verify_remote(tag, current, files, notes, identity, compare_local=False)
        return dict(published="false", url=current["url"], name=current["name"], body=notes)
    prerelease = "true" if "-" in version else "false"
    title = f"{component} {version}"
    if current is None:
        gh("release", "create", tag, "--verify-tag", "--target", source,
           "--draft", "--prerelease=" + prerelease, "--latest=false",
           "--title", title, "--notes-file", str(notes_file))
    else:
        if any(asset["name"] not in files for asset in current["assets"]):
            raise ValueError("Existing draft contains unexpected assets; refusing to replace them")
        gh("release", "edit", tag, "--title", title, "--notes-file", str(notes_file),
           "--prerelease=" + prerelease, "--latest=false")
    # Only a draft may receive replacements when recovering an interrupted run.
    if not release(tag)["isDraft"]:
        raise RuntimeError("Release became published before uploading; refusing to replace assets")
    gh("release", "upload", tag, *[str(files[name]) for name in sorted(files)], "--clobber")
    draft = release(tag)
    if not draft["isDraft"]:
        raise RuntimeError("Release became published during upload; refusing further changes")
    verify_remote(tag, draft, files, notes, identity, compare_local=True)
    gh("release", "edit", tag, "--draft=false", "--prerelease=" + prerelease, "--latest=false")
    published = release(tag)
    if published["isDraft"]:
        raise RuntimeError("GitHub did not publish the verified draft")
    return dict(published="true", url=published["url"], name=title, body=notes)


def outputs(path, values):
    with path.open("a", encoding="utf-8") as stream:
        for key, value in values.items():
            delimiter = "OSG_" + uuid.uuid4().hex
            stream.write(f"{key}<<{delimiter}\n{value}\n{delimiter}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--component", required=True, choices=COMPONENTS)
    parser.add_argument("--version", required=True)
    parser.add_argument("--notes", required=True, type=Path)
    parser.add_argument("--assets", required=True, type=Path)
    parser.add_argument("--github-output", type=Path, default=os.environ.get("GITHUB_OUTPUT"))
    args = parser.parse_args()
    result = publish(args.tag, args.component, args.version, args.notes, args.assets)
    if args.github_output:
        outputs(args.github_output, result)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as stream:
            verb = "Published" if result["published"] == "true" else "Verified existing release"
            stream.write(f"{verb}: [{args.tag}]({result['url']})\n")


if __name__ == "__main__":
    main()
