#!/usr/bin/env python3
"""Summarize supplied capture-to-successful-submit evidence; never qualify a backend."""
import argparse
import json
import math
from pathlib import Path

MAX_BYTES = 32 * 1024 * 1024


def integer(value, label):
    if type(value) is not int or not 0 <= value < 2**63:
        raise ValueError(f'{label} must be a nonnegative bounded integer')
    return value


def percentile(values, percent):
    """Nearest rank, including the maximum when the sample is small."""
    return sorted(values)[math.ceil(len(values) * percent / 100) - 1]


def summarize(data):
    if data.get('schema') != 1 or data.get('backend') != 'vjoy':
        raise ValueError('Expected schema 1 capture-to-vJoy evidence')
    if data.get('clock') != 'host-monotonic-us':
        raise ValueError('Capture and successful submit must share one monotonic microsecond clock')
    if data.get('evidenceKind') not in ('physical', 'authored'):
        raise ValueError('Declare physical or authored provenance')
    metadata = data.get('machine', {})
    for field in ('os', 'cpu', 'appCommit', 'driverVersion', 'game'):
        if not isinstance(metadata.get(field), str) or not 1 <= len(metadata[field]) <= 256:
            raise ValueError(f'Missing bounded machine field: {field}')
    devices = data.get('devices', [])
    if not isinstance(devices, list) or not 1 <= len(devices) <= 256:
        raise ValueError('Record exact source devices')
    sources = set()
    for device in devices:
        for field in ('source', 'model', 'firmware', 'connection'):
            if not isinstance(device.get(field), str) or not 1 <= len(device[field]) <= 256:
                raise ValueError(f'Missing bounded device field: {field}')
        if device['source'] in sources:
            raise ValueError('Duplicate device source')
        sources.add(device['source'])
    diagnostics = data.get('diagnostics', {})
    for key in ('queueCapacity', 'queueHighWater', 'coalescedAxisReports', 'queueGaps',
                'missedTransitions', 'feederFaults', 'submitFailures'):
        integer(diagnostics.get(key), key)
    if not diagnostics['queueCapacity'] or diagnostics['queueHighWater'] > diagnostics['queueCapacity']:
        raise ValueError('Queue high-water mark exceeds capacity')
    duration = integer(data.get('durationUs'), 'durationUs')
    if not duration:
        raise ValueError('Record the observation duration')
    samples = data.get('samples', [])
    if not isinstance(samples, list) or not 1 <= len(samples) <= 100_000:
        raise ValueError('Expected 1..100000 successful submission samples')
    groups = {'builtin': [], 'script': []}
    previous = {}
    excluded = 0
    axis_count = 0
    for sample in samples:
        source = sample.get('source')
        kind = sample.get('kind')
        if source not in sources or kind not in groups or sample.get('control') not in ('axis', 'button', 'hat'):
            raise ValueError('Unknown sample source/kind/control')
        if sample.get('successfulSubmit') is not True:
            raise ValueError('Only successful native submissions have a latency; record failures separately')
        captured = integer(sample.get('capturedUs'), 'capturedUs')
        submitted = integer(sample.get('submittedUs'), 'submittedUs')
        sequence = integer(sample.get('sequence'), 'sequence')
        delay = integer(sample.get('configuredDelayUs'), 'configuredDelayUs')
        if captured > submitted or submitted > duration:
            raise ValueError('Timestamps must be run-relative and ordered within the observation')
        if source in previous:
            old_seq, old_capture, old_submit = previous[source]
            if sequence <= old_seq or captured < old_capture or submitted < old_submit:
                raise ValueError('Per-source sequences/timestamps must advance without reordering')
        previous[source] = sequence, captured, submitted
        axis_count += sample['control'] == 'axis'
        if delay:
            excluded += 1
        else:
            groups[kind].append(submitted - captured)
    statistics = {}
    for kind, values in groups.items():
        statistics[kind] = None if not values else dict(
            count=len(values), p50Us=percentile(values, 50), p95Us=percentile(values, 95),
            p99Us=percentile(values, 99), maximumUs=max(values))
    builtins = statistics['builtin']
    faults = any(diagnostics[key] for key in ('queueGaps', 'missedTransitions', 'feederFaults', 'submitFailures'))
    return dict(schema=1, qualification=False, evidenceKind=data['evidenceKind'], machine=metadata,
                observedSources=len(previous), durationUs=duration, successfulSamples=len(samples),
                submittedAxisReportsPerSecond=round(axis_count * 1_000_000 / duration, 2),
                excludedConfiguredDelaySamples=excluded, latency=statistics, diagnostics=diagnostics,
                latencyTargetsMet=bool(builtins and builtins['p95Us'] < 5000 and builtins['p99Us'] < 10000),
                failureFree=not faults,
                note='Targets compare only supplied successful, undelayed submissions. Authored evidence, '
                     'small samples, missing traffic and failed submissions cannot establish hardware qualification.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    with args.evidence.open('rb') as stream:
        raw = stream.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        parser.error('Evidence exceeds 32 MiB')
    try:
        result = summarize(json.loads(raw))
    except (ValueError, TypeError, AttributeError, KeyError) as error:
        parser.error(str(error))
    text = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding='utf-8')
    print(text, end='')


if __name__ == '__main__':
    main()
