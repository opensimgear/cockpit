"""Read-only Linux readiness probe. Does not grab devices, create outputs, or alter ACLs."""
import json
import os
from pathlib import Path
import platform
import subprocess

result = {"kernel": platform.release(), "architecture": platform.machine(),
          "osRelease": Path("/etc/os-release").read_text(),
          "sessionType": os.getenv("XDG_SESSION_TYPE"),
          "physicalEventNodes": [str(p) for p in Path("/dev/input").glob("event*")],
          "virtualCreationQualified": False, "controlledLaunchQualified": False}
try:
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
except OSError as error:
    result["uinput"] = {"readiness": "setup-required", "errno": error.errno,
                        "diagnostic": str(error)}
else:
    os.close(fd)
    result["uinput"] = {"readiness": "accessible-unqualified"}
try:
    probe = subprocess.run(["unshare", "--user", "--map-root-user", "--mount",
                            "--fork", "/bin/true"], capture_output=True, text=True, timeout=5)
    result["mountNamespace"] = {"exitCode": probe.returncode, "diagnostic": probe.stderr[:2048]}
except (OSError, subprocess.TimeoutExpired) as error:
    result["mountNamespace"] = {"diagnostic": str(error)}
print(json.dumps(result, indent=2))
