"""Executable M0 contract experiments; not a production controller service."""
from dataclasses import dataclass
import json
import ntpath

DOCUMENT_LIMIT = 65_536
FRAME_LIMIT = DOCUMENT_LIMIT + 1_024
U64_MAX = (1 << 64) - 1


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate field")
        result[key] = value
    return result


def decode_frame(data):
    if not isinstance(data, bytes) or len(data) > FRAME_LIMIT:
        raise ValueError("frame size")
    try:
        frame = json.loads(data.decode("utf-8"), object_pairs_hook=_object,
                           parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite")))
        if not isinstance(frame, dict) or set(frame) != {"schema", "requestId", "document"}:
            raise ValueError("envelope fields")
        if type(frame["schema"]) is not int or frame["schema"] != 1:
            raise ValueError("envelope version")
        request_id = frame["requestId"]
        if not isinstance(request_id, str) or not 1 <= len(request_id.encode("utf-8")) <= 80:
            raise ValueError("request ID")
        if not isinstance(frame["document"], dict):
            raise ValueError("document object")
        document = json.dumps(frame["document"], separators=(",", ":"), ensure_ascii=False,
                              allow_nan=False).encode("utf-8")
        if len(document) > DOCUMENT_LIMIT:
            raise ValueError("document size")
        return frame
    except (UnicodeError, RecursionError) as error:
        raise ValueError("invalid encoding or nesting") from error


def counter(value):
    # Decimal strings preserve all u64 values through JavaScript transports.
    if not isinstance(value, str) or not value.isascii() or not value.isdecimal():
        raise ValueError("counter encoding")
    if value != str(int(value)) or not 0 < int(value) <= U64_MAX:
        raise ValueError("counter range")
    return int(value)


@dataclass(frozen=True)
class Integration:
    enabled: bool = False
    readiness: str = "missing"
    capabilities: frozenset = frozenset()

    def require(self, operation):
        if not self.enabled:
            raise ValueError("feature disabled")
        if self.readiness != "ready":
            raise ValueError(self.readiness)
        if operation not in self.capabilities:
            raise ValueError("unsupported capability")
        return operation


class SourceStream:
    """Atomic reports, increasing incarnations, and no timeout for stationary inputs."""
    def __init__(self, source, controls):
        if not source or not controls or len(controls) > 256:
            raise ValueError("source capabilities")
        self.source = source
        self.controls = frozenset(controls)
        self.generation = 0
        self.sequence = 0
        self.values = {}
        self.armed = False

    def accept(self, source, generation, sequence, reason, changes):
        gen, seq = counter(generation), counter(sequence)
        if source != self.source or gen < self.generation:
            raise ValueError("stale or wrong source")
        if not isinstance(changes, dict) or not changes or not set(changes) <= self.controls:
            raise ValueError("controls")
        if any(type(value) is not int or not -32768 <= value <= 32767 for value in changes.values()):
            raise ValueError("values")
        if reason == "baseline":
            if gen <= self.generation or set(changes) != self.controls:
                raise ValueError("fresh complete baseline required")
        elif reason == "change":
            if gen != self.generation or not self.armed:
                raise ValueError("baseline required")
            if seq <= self.sequence:
                raise ValueError("stale sequence")
            if seq != self.sequence + 1:
                self.armed = False
                self.values.clear()
                raise ValueError("gap; release contributions and acquire baseline")
        else:
            raise ValueError("reason")
        self.generation, self.sequence = gen, seq
        self.values.update(changes)
        self.armed = True
        return dict(self.values)

    def disconnect(self):
        self.armed = False
        self.values.clear()


def executable(path):
    if not isinstance(path, str) or not path or "\0" in path:
        raise ValueError("executable path")
    path = path.replace("/", "\\")
    drive, tail = ntpath.splitdrive(path)
    if not drive or not tail.startswith("\\") or ".." in tail.split("\\"):
        raise ValueError("absolute canonical executable required")
    # Production must resolve links and obtain the final existing executable path first.
    return ntpath.normcase(ntpath.normpath(path))


@dataclass(frozen=True)
class HideState:
    active: bool
    inverse: bool
    applications: frozenset
    hidden: frozenset


@dataclass(frozen=True)
class HideChange:
    before: HideState
    after: HideState

    def restore(self, current):
        if current != self.after:
            raise ValueError("external changes; preserve current policy and report conflict")
        return self.before


def plan_hiding(current, selections, capture_executable):
    """Conservative exact global-policy validator; never writes registry/driver settings."""
    capture = executable(capture_executable)
    if not selections:
        return HideChange(current, current)
    if len(selections) > 16:
        raise ValueError("selection limit")
    devices = set()
    policy = None
    for device, mode, process in selections:
        if not device or device in devices or device.startswith("cockpit-virtual:"):
            raise ValueError("unique physical device required")
        devices.add(device)
        if mode == "hide-all":
            if process is not None:
                raise ValueError("unexpected process")
            desired = (False, frozenset({capture}) | (current.applications if not current.inverse else frozenset()))
        elif mode == "hide-from-process":
            target = executable(process)
            if target == capture:
                raise ValueError("capture access would be lost")
            desired = (True, frozenset({target}))
        else:
            raise ValueError("unsupported visibility mode")
        if policy is not None and policy != desired:
            raise ValueError("incompatible global policies")
        policy = desired
    inverse, applications = policy
    if current.hidden and (not current.active or current.inverse != inverse
                           or current.applications != applications):
        raise ValueError("would alter an externally hidden device policy")
    after = HideState(True, inverse, applications, current.hidden | frozenset(devices))
    return HideChange(current, after)
