#!/usr/bin/env python3
"""Build and verify native signing-disabled Companion development installers. Never publishes or installs."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import stat
import struct
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "apps/companion"
FORMATS = {"windows": "nsis", "linux": "deb", "macos": "app"}
FORBIDDEN = {"osg-cockpit", "osg-controller-input", "osg-virtual-controller", "osg-mapping-host", "osg-mapping", "osg-logic"}
MAX_ARTIFACT = 512 * 1024 * 1024
MAX_MANIFEST = 1024 * 1024
CONTRACT_FILES = (
    "crates/companion-service/src/server.rs", "crates/companion-service/src/client.rs",
    "crates/companion-service/src/tls.rs", "crates/companion-service/src/persistence.rs",
    "crates/companion-service/src/hub.rs", "crates/companion-service/src/hub_feedback.rs",
    "crates/companion-service/src/hub_assignments.rs", "crates/companion-service/src/publication.rs",
    "crates/companion-service/src/feedback.rs", "crates/companion-platform/src/lib.rs",
    "packages/companion-contracts/src/contracts.ts", "packages/companion-designer/src/types.ts",
    "packages/companion-designer/src/model.ts", "apps/companion/src/lib/companion-cache.ts",
)
DIGEST = re.compile(r"[a-f0-9]{64}\Z")
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")


def canonical_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")


def digest_value(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def package_identity(config):
    version, identifier, product = (config.get(key) for key in ("version", "identifier", "productName"))
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise ValueError("Companion requires an explicit semantic package version.")
    if identifier != "org.opensimgear.companion" or product != "OpenSimGear Companion":
        raise ValueError("Unexpected standalone Companion application identity.")
    if config.get("bundle", {}).get("createUpdaterArtifacts") is not False:
        raise ValueError("Development packaging must keep updater artifacts disabled.")
    return {"identifier": identifier, "productName": product, "version": version}


def validate_versions(identity, root=ROOT):
    cargo = tomllib.loads((root / "apps/companion/src-tauri/Cargo.toml").read_text(encoding="utf-8"))
    frontend = json.loads((root / "apps/companion/package.json").read_text(encoding="utf-8"))
    if cargo["package"]["version"] != identity["version"] or frontend["version"] != identity["version"]:
        raise ValueError("Companion Tauri, Rust and frontend package versions must agree.")


def source_provenance(root=ROOT):
    """Hash build inputs without relying on an enclosing checkout's misleading HEAD."""
    roots = ["apps/companion", "packages/companion-contracts", "packages/companion-designer",
             "packages/companion-ui", "crates/companion-service", "crates/companion-platform"]
    paths = [root / name for name in ("pnpm-lock.yaml", "pnpm-workspace.yaml", "package.json", "rust-toolchain.toml", "LICENSE")]
    ignored = {"node_modules", "dist", "target", "gen", ".svelte-kit", "__pycache__"}
    for name in roots:
        for directory, children, files in os.walk(root / name, followlinks=False):
            children[:] = sorted(child for child in children if child not in ignored)
            paths.extend(Path(directory) / file for file in files)
    inputs = {}
    for path in sorted(paths):
        if path.is_symlink() or not path.is_file():
            raise ValueError("Companion build inputs must be regular files.")
        inputs[path.relative_to(root).as_posix()] = sha256(path)
    # A source archive intentionally has no authenticated commit identity. Never inherit its parent's .git.
    commit = None
    if (root / ".git").exists():
        commit = run(["git", "rev-parse", "HEAD"], cwd=root, capture=True)
        if not re.fullmatch(r"[a-f0-9]{40}", commit):
            raise ValueError("Invalid source revision.")
    return {"commit": commit, "buildInputsSha256": digest_value(inputs), "inputCount": len(inputs)}


def compatibility_contract(root=ROOT):
    sources = {name: sha256(root / name) for name in CONTRACT_FILES}
    server = (root / CONTRACT_FILES[0]).read_text(encoding="utf-8")
    match = re.search(r'pub const SUBPROTOCOL: &str = "([A-Za-z0-9-]+)";', server)
    if not match:
        raise ValueError("Companion subprotocol declaration is missing.")
    return {"wireVersion": 1, "websocketSubprotocol": match.group(1), "pairingPayloadVersion": 1,
            "portableUiSchema": 1, "designerDocumentSchema": 2, "trustModel": "host-ca-spki-sha256",
            "credentialMigration": "native-store-review-required", "contractSources": sources,
            "contractSourcesSha256": digest_value(sources)}


def strict_keys(value, required):
    if not isinstance(value, dict) or set(value) != set(required):
        raise ValueError("Invalid development evidence fields.")


def valid_digest(value):
    return isinstance(value, str) and DIGEST.fullmatch(value) is not None


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
    if not isinstance(system, str) or system not in endings or not isinstance(target, str) or target not in {arch + endings[system] for arch in ("x86_64", "aarch64")} :
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


def verify_artifact(path, kind, identity=None, target=None):
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size <= MAX_ARTIFACT:
        raise ValueError("Package must be a bounded regular file.")
    metadata = None
    if kind == "nsis":
        with path.open("rb") as handle:
            header = handle.read(64)
            if path.suffix.lower() != ".exe" or not header.startswith(b"MZ"):
                raise ValueError("Expected an NSIS Windows executable.")
            if identity:
                if len(header) != 64:
                    raise ValueError("Windows installer has no bounded PE header.")
                offset = struct.unpack_from("<I", header, 60)[0]
                if not 64 <= offset <= min(path.stat().st_size - 24, 1024 * 1024):
                    raise ValueError("Windows installer has an invalid PE offset.")
                handle.seek(offset)
                pe = handle.read(24)
                if pe[:4] != b"PE\0\0" or struct.unpack_from("<H", pe, 4)[0] not in {0x14c, 0x8664, 0xaa64}:
                    raise ValueError("Windows installer has an invalid PE machine.")
                arch = {"x86_64": "x64", "aarch64": "arm64"}[target.split("-")[0]]
                if not path.name.endswith(f"_{identity['version']}_{arch}-setup.exe"):
                    raise ValueError("Windows installer filename does not match version/target.")
                # NSIS may use an x86 stub for a 64-bit payload. Do not infer installed payload architecture.
                metadata = {"version": identity["version"], "target": target, "verification": "pe-and-filename-only"}
    elif kind == "deb":
        fields = run(["dpkg-deb", "--field", path, "Package", "Version", "Architecture"], capture=True)
        contents = run(["dpkg-deb", "--contents", path], capture=True)
        paths = [line.split()[-1].removeprefix("./") for line in contents.splitlines() if line.split()]
        if "Package: open-sim-gear-companion" not in fields or "usr/bin/osg-companion" not in paths:
            raise ValueError("Debian package is missing the standalone Companion executable.")
        if any(name in contents for name in ["osg-gateway-service", "osg-cockpit", "vJoyInterface.dll"]):
            raise ValueError("Debian package includes an unexpected host/driver executable.")
        if identity:
            parsed = dict(line.split(": ", 1) for line in fields.splitlines() if ": " in line)
            architecture = {"x86_64": "amd64", "aarch64": "arm64"}[target.split("-")[0]]
            if parsed.get("Version") != identity["version"] or parsed.get("Architecture") != architecture:
                raise ValueError("Debian package version/architecture does not match the build contract.")
            metadata = {"version": parsed["Version"], "architecture": architecture, "verification": "dpkg-metadata-and-contents"}
    elif kind == "app-zip":
        with zipfile.ZipFile(path) as archive:
            items = archive.infolist()
            if len(items) > 8192 or sum(item.file_size for item in items) > MAX_ARTIFACT:
                raise ValueError("macOS bundle exceeds verification budget.")
            names = [item.filename for item in items]
            if len(set(names)) != len(names):
                raise ValueError("macOS archive contains duplicate records.")
            for item in items:
                parts = item.filename.split("/")
                if ("\\" in item.orig_filename or "\0" in item.orig_filename or not item.filename or item.filename.startswith("/") or "\\" in item.filename
                        or any(part in {".", ".."} or ":" in part for part in parts) or "\0" in item.filename):
                    raise ValueError("macOS archive contains an escaping path.")
                if stat.S_ISLNK(item.external_attr >> 16):
                    raise ValueError("macOS development archive must not contain symlinks.")
            infos = [item for item in items if item.filename.endswith(".app/Contents/Info.plist")]
            if len(infos) != 1 or infos[0].file_size > 1024 * 1024:
                raise ValueError("macOS archive is missing unique native app metadata.")
            info = infos[0]
            app_root = info.filename.removesuffix("Contents/Info.plist")
            binaries = [item for item in items if item.filename == app_root + "Contents/MacOS/osg-companion"]
            if len(binaries) != 1 or binaries[0].file_size < 16:
                raise ValueError("macOS archive is missing its matching executable.")
            plist = plistlib.loads(archive.read(info))
            if plist.get("CFBundleIdentifier") != "org.opensimgear.companion":
                raise ValueError("Unexpected macOS bundle identity.")
            if identity:
                if (plist.get("CFBundleShortVersionString") != identity["version"]
                        or plist.get("CFBundleExecutable") != "osg-companion"):
                    raise ValueError("macOS bundle version/executable does not match the build contract.")
                with archive.open(binaries[0]) as binary:
                    header = binary.read(4096)
                architecture = macho_architecture(header)
                expected = target.split("-")[0]
                if expected not in architecture:
                    raise ValueError("macOS executable architecture does not match the build target.")
                metadata = {"version": plist["CFBundleShortVersionString"], "architectures": architecture,
                            "verification": "plist-and-macho-header"}

    else:
        raise ValueError("Unknown package format.")
    result = {"path": path.name, "format": kind, "bytes": path.stat().st_size, "sha256": sha256(path)}
    if metadata:
        result["metadata"] = metadata
    return result


def macho_architecture(header):
    cpus = {0x01000007: "x86_64", 0x0100000c: "aarch64"}
    if len(header) >= 16 and header[:4] in {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"}:
        endian = "<" if header[:4] == b"\xcf\xfa\xed\xfe" else ">"
        cpu = struct.unpack_from(endian + "I", header, 4)[0]
        if cpu in cpus:
            return [cpus[cpu]]
    elif len(header) >= 8 and header[:4] == b"\xca\xfe\xba\xbe":
        count = struct.unpack_from(">I", header, 4)[0]
        if 1 <= count <= 8 and len(header) >= 8 + count * 20:
            result = [cpus.get(struct.unpack_from(">I", header, 8 + index * 20)[0]) for index in range(count)]
            if all(result) and len(set(result)) == len(result):
                return sorted(result)
    raise ValueError("Unsupported or malformed 64-bit Mach-O header.")


def verify_manifest(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_MANIFEST:
        raise ValueError("Invalid development evidence file.")
    evidence = json.loads(path.read_text(encoding="utf-8"))
    if (not isinstance(evidence, dict) or type(evidence.get("schemaVersion")) is not int or evidence.get("schemaVersion") not in {1, 2} or evidence.get("buildKind") != "development"
            or evidence.get("signingRequested") is not False or evidence.get("published") is not False
            or evidence.get("installed") is not False or not isinstance(evidence.get("files"), list) or not 1 <= len(evidence["files"]) <= 8):
        raise ValueError("Expected signing-disabled, uninstalled development evidence.")
    if evidence["schemaVersion"] == 2:
        validate_evidence_contract(evidence)
    root = path.parent.resolve()
    names = set()
    for item in evidence["files"]:
        if not isinstance(item, dict) or not isinstance(item.get("path"), str) or item["path"] in names:
            raise ValueError("Invalid or duplicate package entry.")
        names.add(item["path"])
        source = root / item["path"]
        artifact = source.resolve()
        if source.is_symlink() or artifact.parent != root or Path(item["path"]).name != item["path"] or "\\" in item["path"] or ":" in item["path"]:
            raise ValueError("Package path escapes its evidence directory.")
        if evidence["schemaVersion"] == 2:
            kind = {"windows": "nsis", "linux": "deb", "macos": "app-zip"}[evidence["platform"]]
            if item.get("format") != kind:
                raise ValueError("Package format does not match the native platform.")
            measured = verify_artifact(artifact, kind, evidence["application"], evidence["target"])
        else:
            measured = verify_artifact(artifact, item.get("format"))
        if measured != item:
            raise ValueError("Package bytes do not match development evidence.")
    return evidence


def validate_evidence_contract(evidence):
    strict_keys(evidence, {"schemaVersion", "buildKind", "signingRequested", "published", "installed", "platform", "target",
                           "builtAtUtc", "commands", "dependencies", "files", "application", "source", "compatibility",
                           "configSha256", "compatibilitySha256", "releaseQualified", "signatureVerification"})
    if evidence["releaseQualified"] is not False or evidence["signatureVerification"] != "not-performed":
        raise ValueError("Development evidence cannot claim signature/release qualification.")
    validate_target(evidence["target"], evidence["platform"])
    identity = evidence["application"]
    strict_keys(identity, {"identifier", "productName", "version"})
    package_identity({**identity, "bundle": {"createUpdaterArtifacts": False}})
    if evidence["commands"] != build_commands(evidence["target"], evidence["platform"]):
        raise ValueError("Development commands differ from the locked unsigned plan.")
    names = evidence["dependencies"]
    if (not isinstance(names, list) or any(not isinstance(name, str) for name in names)
            or names != sorted(set(names)) or FORBIDDEN.intersection(names)
            or not {"osg-companion", "osg-companion-service"}.issubset(names)):
        raise ValueError("Invalid standalone Companion dependency evidence.")
    source = evidence["source"]
    strict_keys(source, {"commit", "buildInputsSha256", "inputCount"})
    if (source["commit"] is not None and not re.fullmatch(r"[a-f0-9]{40}", str(source["commit"]))) or not valid_digest(source["buildInputsSha256"]) or type(source["inputCount"]) is not int or source["inputCount"] < 1:
        raise ValueError("Invalid source provenance.")
    contract = evidence["compatibility"]
    strict_keys(contract, {"wireVersion", "websocketSubprotocol", "pairingPayloadVersion", "portableUiSchema", "designerDocumentSchema", "trustModel", "credentialMigration", "contractSources", "contractSourcesSha256"})
    if any(type(contract[key]) is not int or contract[key] < 1 for key in ("wireVersion", "pairingPayloadVersion", "portableUiSchema", "designerDocumentSchema")):
        raise ValueError("Invalid compatibility versions.")
    if (not isinstance(contract["websocketSubprotocol"], str) or not re.fullmatch(r"[A-Za-z0-9-]{1,80}", contract["websocketSubprotocol"])
            or contract["trustModel"] != "host-ca-spki-sha256" or contract["credentialMigration"] != "native-store-review-required"):
        raise ValueError("Invalid trust/protocol declaration.")
    hashes = contract["contractSources"]
    if (not isinstance(hashes, dict) or set(hashes) != set(CONTRACT_FILES) or not all(valid_digest(value) for value in hashes.values())
            or contract["contractSourcesSha256"] != digest_value(hashes)
            or evidence["compatibilitySha256"] != digest_value(contract) or not valid_digest(evidence["configSha256"])):
        raise ValueError("Compatibility/source digest does not match its metadata.")
    try:
        stamp = datetime.fromisoformat(evidence["builtAtUtc"])
        if stamp.tzinfo is None or stamp.utcoffset().total_seconds() != 0:
            raise ValueError("UTC required")
    except (TypeError, ValueError):
        raise ValueError("Build timestamp must be explicit UTC.") from None


def compare_evidence(previous, current):
    """Metadata comparison only: cannot prove install/update, signatures or trust migration."""
    if previous.get("schemaVersion") != 2 or current.get("schemaVersion") != 2:
        raise ValueError("Legacy evidence has no compatibility contract; rebuild before comparison.")
    validate_evidence_contract(previous)
    validate_evidence_contract(current)
    reasons = []
    for key in ("platform", "target"):
        if previous[key] != current[key]:
            reasons.append(key + " changed")
    if previous["application"]["identifier"] != current["application"]["identifier"]:
        reasons.append("application identity changed")
    if previous["compatibility"] != current["compatibility"]:
        reasons.append("protocol/document/trust contract or implementation changed; review pairing/publication migration")
    version = lambda value: tuple(map(int, value.split("-")[0].split("+")[0].split(".")))
    old_version, new_version = previous["application"]["version"], current["application"]["version"]
    if version(new_version) < version(old_version):
        reasons.append("application version downgrade")
    if old_version != new_version and ("-" in old_version or "-" in new_version):
        reasons.append("prerelease version change")
    return {"schemaVersion": 1, "scope": "metadata-only", "classification": "review-required" if reasons else "metadata-compatible",
            "reasons": reasons, "nativeInstallUpdateQualified": False, "releaseQualified": False,
            "trustMigrationVerified": False, "automaticUpdateEnabled": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / ".research/companion-package")
    parser.add_argument("--target", help="Native Rust target; defaults to rustc's host")
    parser.add_argument("--print-plan", action="store_true")
    parser.add_argument("--verify-only", type=Path, metavar="EVIDENCE_JSON")
    parser.add_argument("--compare-to", type=Path, metavar="PREVIOUS_EVIDENCE_JSON", help="With --verify-only, compare metadata; never qualifies native updates")
    options = parser.parse_args()
    if options.compare_to and not options.verify_only:
        raise ValueError("--compare-to requires --verify-only for the new evidence.")
    if options.verify_only:
        evidence = verify_manifest(options.verify_only.resolve())
        if options.compare_to:
            print(json.dumps(compare_evidence(verify_manifest(options.compare_to.resolve()), evidence), sort_keys=True, indent=2))
        else:
            print(f"Verified {len(evidence['files'])} development package(s); no signature, install, update or runtime qualification.")
        return
    system = native_platform()
    if system not in FORMATS:
        raise ValueError("Desktop packaging requires a native Windows, Linux or macOS host.")
    host = next(line[6:] for line in run(["rustc", "-vV"], capture=True).splitlines() if line.startswith("host: "))
    target = options.target or host
    if target != host:
        raise ValueError("Native package verification requires rustc's exact host target.")
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
    config_path = APP / "src-tauri/tauri.conf.json"
    config = json.loads(config_path.read_text(encoding="utf-8"))
    identity = package_identity(config)
    validate_versions(identity)
    provenance = source_provenance()
    contract = compatibility_contract()
    for command in commands:
        run(command, environment=environment)
    bundle = target_dir / target / "debug/bundle" / {"windows": "nsis", "linux": "deb", "macos": "macos"}[system]
    if source_provenance() != provenance:
        raise ValueError("Companion build inputs changed during packaging; rebuild from a stable source tree.")
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
    evidence = {"schemaVersion": 2, "buildKind": "development", "signingRequested": False,
                "published": False, "installed": False, "releaseQualified": False, "signatureVerification": "not-performed", "platform": system, "target": target,
                "application": identity, "source": provenance, "compatibility": contract,
                "configSha256": sha256(config_path), "compatibilitySha256": digest_value(contract),
                "builtAtUtc": datetime.now(timezone.utc).isoformat(), "commands": commands,
                "dependencies": names, "files": [verify_artifact(artifact, kind, identity, target)]}
    manifest = output / "development-evidence.json"
    manifest.write_text(json.dumps(evidence, sort_keys=True, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    verify_manifest(manifest)
    print(f"Verified development package (no native update/release qualification): {manifest}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, zipfile.BadZipFile, plistlib.InvalidFileException, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
