#!/usr/bin/env python3
"""Verify installation-specific CA startup and persistence in isolated native processes."""
import argparse
import importlib.util
import json
from pathlib import Path
import socket
import sqlite3
import ssl
import subprocess
import time
from unittest.mock import patch
import uuid

spec = importlib.util.spec_from_file_location(
    "mapping_acceptance", Path(__file__).with_name("mapping-acceptance.py"))
mapping = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mapping)


def start(executable, directory):
    directory.mkdir(parents=True, exist_ok=True)
    database = directory / "cockpit.sqlite3"
    with patch.dict("os.environ", {"OSG_SQLITE_DATABASE": str(database)}):
        service = mapping.Service(executable, directory)
    # Inspect SQLite before any operator request: first native startup owns creation.
    deadline = time.monotonic() + 15
    try:
        while time.monotonic() < deadline:
            if database.exists():
                try:
                    with sqlite3.connect(database) as connection:
                        if connection.execute(
                            "SELECT COUNT(*) FROM records WHERE domain='secrets' "
                            "AND key='companion-identity' AND payload IS NOT NULL"
                        ).fetchone()[0]:
                            return service
                except sqlite3.OperationalError:
                    pass  # The native owner may still be initializing its schema.
            time.sleep(0.05)
        raise AssertionError("First startup did not persist its own CA before operator access")
    except BaseException:
        close(service)
        raise


def close(service):
    service.process.stdin.close()
    try:
        assert service.process.wait(timeout=20) == 0
    except subprocess.TimeoutExpired:
        service.process.kill()
        service.process.wait(timeout=5)
        raise


def command(service, name, **payload):
    reply = service.request(name, companion=payload)
    assert reply["ok"], reply
    return reply["state"]["companions"]


def certificate(service):
    pem = command(service, "companion_browser_certificate")["certificatePem"]
    assert pem.count("-----BEGIN CERTIFICATE-----") == 1
    assert "PRIVATE KEY" not in pem
    return pem


def enable(service):
    revision = command(service, "companion_inventory")["listener"]["settingsRevision"]
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    result = command(service, "companion_enable", address="127.0.0.1",
                     advertisedHost="127.0.0.1", port=port, discovery=False,
                     expectedSettingsRevision=revision)
    assert result["listener"]["enabled"], result["listener"]
    return port


def verified_leaf(port, root):
    context = ssl.create_default_context(cadata=root)
    with socket.create_connection(("127.0.0.1", port), timeout=5) as tcp:
        with context.wrap_socket(tcp, server_hostname="127.0.0.1") as stream:
            leaf = stream.getpeercert(binary_form=True)
            assert leaf != ssl.PEM_cert_to_DER_cert(root)
            return leaf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--service", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path(".research/companion-certificate/processes.json"))
    options = parser.parse_args()
    root = options.output.resolve().parent / ("installations-" + str(uuid.uuid4()))
    first, second = root / "first", root / "second"
    service = start(options.service.resolve(), first)
    try:
        ca = certificate(service)
        port = enable(service)
        initial_leaf = verified_leaf(port, ca)
        state = command(service, "companion_inventory")
        command(service, "companion_disable",
                expectedSettingsRevision=state["listener"]["settingsRevision"])
        assert certificate(service) == ca
        port = enable(service)
        assert certificate(service) == ca
        assert verified_leaf(port, ca) != initial_leaf
    finally:
        close(service)
    restarted = start(options.service.resolve(), first)
    try:
        assert certificate(restarted) == ca
        state = command(restarted, "companion_inventory")
        assert state["listener"]["enabled"]
        assert verified_leaf(port, ca) != initial_leaf
    finally:
        close(restarted)
    independent = start(options.service.resolve(), second)
    try:
        assert certificate(independent) != ca
    finally:
        close(independent)
    evidence = dict(firstStartupCreatesCA=True, independentInstallationsDiffer=True,
                    listenerRenewalPreservesRoot=True, processRestartPreservesRoot=True,
                    renewedLeafVerifiedWithOriginalRoot=True)
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence))


if __name__ == "__main__":
    main()
