"""M0 report-boundary/SYN_DROPPED experiment, independent of device permissions."""
class EvdevStream:
    def __init__(self):
        self.generation = 0
        self.current = {}
        self.pending = {}
        self.needs_baseline = True

    def baseline(self, values):
        if not isinstance(values, dict) or not values or len(values) > 256:
            raise ValueError("bounded baseline required")
        self.generation += 1
        self.current = dict(values)
        self.pending.clear()
        self.needs_baseline = False
        return self.generation, dict(self.current)

    def event(self, kind, code, value):
        if kind == 0 and code == 3:  # EV_SYN/SYN_DROPPED
            self.current.clear()
            self.pending.clear()
            self.needs_baseline = True
            return "release-and-rebaseline"
        if self.needs_baseline:
            return None
        if kind in (1, 3):  # EV_KEY/EV_ABS
            if (kind, code) not in self.current:
                raise ValueError("unadvertised control")
            self.pending[(kind, code)] = value
        elif kind == 0 and code == 0:  # EV_SYN/SYN_REPORT
            self.current.update(self.pending)
            self.pending.clear()
            return self.generation, dict(self.current)
        return None
