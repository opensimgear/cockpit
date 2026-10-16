#!/usr/bin/env python3
"""Validate the development matrix and disabled gates; never authorize a release."""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
ISSUES = [54, 55, 56, 57, 58, 59, 60, 61, 62, 66, 67, 68]
CLIENTS = {
    "windows": ("development-only", "nsis", "native-protected-store", [57, 59, 60, 68]),
    "linux": ("development-only", "deb", "native-protected-store", [61, 68]),
    "macos": ("development-only", "app-zip", "native-protected-store", [62, 68]),
    "android": ("development-only", "debug-apk", "disabled-pending-qualification", [55, 59, 68]),
    "ios": ("candidate", "unqualified-xcode-build", "disabled-pending-qualification", [56, 59, 68]),
}
BUDGETS = {
    "activeCompanions": 8, "pagesPerUi": 16, "controlsPerUi": 128, "feedbackChannelsPerUi": 64,
    "inputFrameBytes": 16384, "changesPerBatch": 64, "inputBatchesPerSecond": 120,
    "queuedTransitionsPerClient": 512, "feedbackUpdatesPerSecond": 30,
    "heartbeatMs": 250, "hostLeaseMs": 1500, "feedbackStaleMs": 1000,
    "sharedSourceRoles": 16, "sharedTargets": 8, "sharedNodes": 256,
    "releaseLatencyTargetP95Ms": 25, "releaseLatencyTargetP99Ms": 50, "releaseFeedbackTargetMs": 250,
}
# These are repository consistency checks, not runtime/native qualification.
GATES = {
    "host-live-output": ("crates/mapping-host/src/service.rs",
        r"pub fn new\([\s\S]*?Self::with_output\([\s\S]*?osg_virtual_controller::entry::factory\(\),\s*false,\s*\)"),
    "windows-provisioning": ("crates/virtual-controller/src/windows_launcher.rs", r"^pub const QUALIFIED: bool = false;$"),
    "windows-hiding": ("crates/controller-input/src/visibility/windows.rs", r"^pub const QUALIFIED: bool = false;$"),
    "live-scripts": ("crates/mapping-host/src/service.rs", r"^pub const SCRIPT_LIVE_QUALIFIED: bool = false;$"),
    "linux-output": ("crates/virtual-controller/src/uinput.rs", r"^pub const QUALIFIED: bool = false;$"),
    "macos-output": ("crates/virtual-controller/src/apple_helper.rs", r"^pub const QUALIFIED: bool = false;$"),
    "linux-isolated-launch": ("crates/controller-input/src/visibility/launch.rs", r"^pub const QUALIFIED: bool = false;$"),
    "mobile-authentication": ("crates/companion-service/src/client.rs", r"fn authentication_reason\([\s\S]{0,200}?if mobile \{\s*Some\("),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "Duplicate matrix field: " + key)
        value[key] = item
    return value


def load_matrix(path):
    require(not path.is_symlink() and path.is_file() and path.stat().st_size <= 65536,
            "Matrix must be a bounded regular file.")
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)


def validate(matrix, root=ROOT):
    fields = {"schemaVersion", "releaseStatus", "releaseQualified", "featureIssue", "implementationIssue",
              "clients", "pendingQualificationIssues", "disabledCapabilities", "budgets", "performanceEvidence"}
    require(isinstance(matrix, dict) and set(matrix) == fields, "Invalid release matrix fields.")
    require(type(matrix["schemaVersion"]) is int and matrix["schemaVersion"] == 1, "Unsupported matrix schema.")
    require(matrix["releaseStatus"] == "blocked" and matrix["releaseQualified"] is False,
            "Development evidence cannot qualify or authorize a release.")
    require(matrix["featureIssue"] == 37 and matrix["implementationIssue"] == 44, "Unexpected milestone ownership.")
    require(matrix["pendingQualificationIssues"] == ISSUES, "Pending qualification stories must remain visible.")
    require(matrix["performanceEvidence"] == "pending-physical-measurement", "Release budgets are not measured results.")
    require(matrix["budgets"] == BUDGETS and all(type(v) is int for v in matrix["budgets"].values()),
            "Budgets differ from the declared C0 release targets.")
    clients = matrix["clients"]
    require(isinstance(clients, dict) and set(clients) == set(CLIENTS), "Declare all five native client targets.")
    for name, expected in CLIENTS.items():
        row = clients[name]
        require(isinstance(row, dict) and set(row) == {"status", "artifact", "candidateFloor", "authentication", "qualificationIssues"},
                "Invalid client matrix: " + name)
        require(tuple(row.get(k) for k in ["status", "artifact", "authentication", "qualificationIssues"]) == expected,
                "Unqualified client cannot become release supported: " + name)
        require(isinstance(row["candidateFloor"], str) and 1 <= len(row["candidateFloor"]) <= 256
                and not any(ord(c) < 32 for c in row["candidateFloor"]), "Invalid candidate floor: " + name)
    disabled = matrix["disabledCapabilities"]
    require(isinstance(disabled, dict) and set(disabled) == set(GATES)
            and all(v is False for v in disabled.values()), "Unqualified capabilities must stay disabled.")
    verified = []
    for name, (relative, pattern) in GATES.items():
        source = (root / relative).read_text(encoding="utf-8")
        require(re.search(pattern, source, re.M) is not None,
                "Native gate changed; retain separate qualification and reviewed enablement: " + name)
        verified.append(name)
    return {"schemaVersion": 1, "releaseEligible": False, "status": "blocked",
            "qualifiedClientTargets": [], "pendingQualificationIssues": ISSUES,
            "verifiedDisabledSourceGates": verified, "budgets": BUDGETS,
            "performanceEvidence": "pending-physical-measurement",
            "note": "Development build/tests and metadata compatibility are not signed/native/game/performance qualification."}


def main(arguments=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", type=Path, default=ROOT / "docs/companion-release-matrix.json")
    parser.add_argument("--root", type=Path, default=ROOT, help="Repository source used for gate consistency checks")
    parser.add_argument("--output", type=Path, help="Write the blocked-readiness evidence JSON")
    parser.add_argument("--require-release", action="store_true", help="Fail while native qualification is outstanding")
    args = parser.parse_args(arguments)
    try:
        report = validate(load_matrix(args.matrix), args.root)
        encoded = json.dumps(report, indent=2) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(encoded, encoding="utf-8")
        print(encoded, end="")
        return 2 if args.require_release else 0
    except (ValueError, OSError, TypeError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())