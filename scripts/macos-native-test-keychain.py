#!/usr/bin/env python3
"""Disposable GitHub-hosted macOS Keychain for real cross-binary native tests."""
import json
import os
from pathlib import Path
import secrets
import shlex
import subprocess
import sys


def security(*arguments):
    result = subprocess.run(["/usr/bin/security", *arguments], capture_output=True, text=True)
    if result.returncode:
        # Never include the command arguments: setup contains an ephemeral key.
        raise RuntimeError(f"Test Keychain {arguments[0]} failed ({result.returncode}).")
    return result.stdout


def main():
    if (sys.platform != "darwin" or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"):
        raise SystemExit("This fixture is restricted to disposable GitHub-hosted macOS runners.")
    root = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    keychain = root / "osg-native-tests.keychain-db"
    record = root / "osg-native-test-keychain-previous.json"
    if sys.argv[1:] == ["setup"]:
        if record.exists() or keychain.exists():
            raise SystemExit("An existing test Keychain fixture must be cleaned up first.")
        previous = {
            "default": shlex.split(security("default-keychain", "-d", "user")),
            "search": shlex.split(security("list-keychains", "-d", "user")),
        }
        record.write_text(json.dumps(previous), encoding="utf-8")
        password = secrets.token_hex(32)
        security("create-keychain", "-p", password, str(keychain))
        security("set-keychain-settings", "-lut", "21600", str(keychain))
        security("unlock-keychain", "-p", password, str(keychain))
        security("list-keychains", "-d", "user", "-s", str(keychain))
        security("default-keychain", "-d", "user", "-s", str(keychain))
        # The normal and instrumented test binaries are distinct Keychain clients.
        # Only this random test item permits both without an interactive ACL prompt.
        # Real AES-GCM, native reads and receipt authentication remain exercised.
        security("add-generic-password", "-a", "aes256-gcm-v1",
                 "-s", "org.opensimgear.companion.native-state",
                 "-X", secrets.token_hex(32), "-A", str(keychain))
        print("Disposable native test Keychain is ready for both test binaries.")
    elif sys.argv[1:] == ["cleanup"]:
        if record.exists():
            previous = json.loads(record.read_text(encoding="utf-8"))
            if previous["default"]:
                security("default-keychain", "-d", "user", "-s", previous["default"][0])
            security("list-keychains", "-d", "user", "-s", *previous["search"])
            if keychain.exists():
                security("delete-keychain", str(keychain))
            record.unlink()
        print("Native test Keychain removed; prior user Keychain settings restored.")
    else:
        raise SystemExit("Usage: macos-native-test-keychain.py setup|cleanup")


if __name__ == "__main__":
    main()
