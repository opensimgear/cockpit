export function selectBaseline({ mode, head, eventBase, runs, canCompare = () => true }) {
  if (!head) throw new Error('Current head SHA is required.');

  if (mode === 'pull_request') {
    if (!eventBase) throw new Error('Pull request base SHA is required.');
    if (!canCompare(eventBase, head)) return { base: '', head, full: true, hasChanges: true };
    return { base: eventBase, head, full: false, hasChanges: eventBase !== head };
  }

  if (mode === 'manual') return { base: '', head, full: true, hasChanges: true };
  if (mode !== 'push' && mode !== 'daily') throw new Error(`Unsupported mode: ${mode}`);

  const previous = runs.find((run) => run.conclusion === 'success');
  if (!previous) {
    return { base: '', head, full: true, hasChanges: true };
  }
  if (!canCompare(previous.head_sha, head)) return { base: '', head, full: true, hasChanges: true };

  return {
    base: previous.head_sha,
    head,
    full: false,
    hasChanges: previous.head_sha !== head,
  };
}

export function selectBaselineWithFallback({ mode, head, eventBase, readRuns, canCompare, onReadError = () => {} }) {
  let runs = [];
  if (mode !== 'pull_request' && mode !== 'manual') {
    try {
      runs = readRuns();
    } catch (error) {
      onReadError(error);
    }
  }
  return selectBaseline({ mode, head, eventBase, runs, canCompare });
}
