"""Bounded Linux native service startup check with no Windows optional dependencies."""
import json
from pathlib import Path
import select
import subprocess

root = Path(__file__).resolve().parents[2]
service = root / ".research/controller-mapping-m0/native-target/debug/osg-gateway-service"
process = subprocess.Popen([str(service)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, text=True)
try:
    process.stdin.write(json.dumps({"requestId": "m0-startup", "request": {"command": "status", "clientId": "m0-startup-probe"}}) + "\n")
    process.stdin.flush()
    if not select.select([process.stdout], [], [], 15)[0]:
        raise RuntimeError("Native service did not answer within 15 seconds")
    response = json.loads(process.stdout.readline())
    assert response["requestId"] == "m0-startup", response
    assert response["response"]["ok"], response
    process.stdin.close()
    process.wait(timeout=15)
    assert process.returncode == 0, process.stderr.read()
    print(json.dumps({"startup": "passed", "response": response}))
finally:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
