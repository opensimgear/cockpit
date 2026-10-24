import test from 'node:test';
import assert from 'node:assert/strict';
import { readSuccessfulRuns, selectBaseline, selectBaselineWithFallback } from './baseline.mjs';

test('pull requests compare with their merge base', () => {
  assert.deepEqual(selectBaseline({ mode: 'pull_request', head: 'merge', eventBase: 'base' }), {
    base: 'base', head: 'merge', full: false, hasChanges: true,
  });
});

test('first run has no baseline and requests a full run', () => {
  assert.deepEqual(selectBaseline({ mode: 'daily', head: 'new', runs: [] }), {
    base: '', head: 'new', full: true, hasChanges: true,
  });
});

test('latest successful run becomes the baseline across a backlog of commits', () => {
  assert.deepEqual(selectBaseline({ mode: 'daily', head: 'latest', runs: [
    { head_sha: 'last-success', conclusion: 'success' },
    { head_sha: 'older', conclusion: 'success' },
  ] }), { base: 'last-success', head: 'latest', full: false, hasChanges: true });
});

test('same successful SHA skips daily work', () => {
  assert.deepEqual(selectBaseline({ mode: 'daily', head: 'same', runs: [
    { head_sha: 'same', conclusion: 'success' },
  ] }), { base: 'same', head: 'same', full: false, hasChanges: false });
});

test('unavailable or non-ancestor baseline falls back to a full run', () => {
  assert.deepEqual(selectBaseline({ mode: 'daily', head: 'current', runs: [
    { head_sha: 'deleted-or-unrelated', conclusion: 'success' },
  ], canCompare: () => false }), {
    base: '', head: 'current', full: true, hasChanges: true,
  });
});

test('GitHub API lookup errors trigger a full run and are reported', () => {
  const warnings = [];
  const selected = selectBaselineWithFallback({
    mode: 'daily',
    head: 'current',
    readRuns: () => { throw new Error('API unavailable'); },
    onReadError: (error) => warnings.push(error.message),
  });
  assert.deepEqual(selected, { base: '', head: 'current', full: true, hasChanges: true });
  assert.deepEqual(warnings, ['API unavailable']);
});

test('GitHub workflow runs are paginated and only successful runs are retained', () => {
  let argsSeen;
  const runs = readSuccessfulRuns({ repo: 'opensimgear/cockpit', workflow: 'daily.yml', branch: 'main', execute: (command, args) => {
    assert.equal(command, 'gh');
    argsSeen = args;
    return 'older-success\tsuccess\t2026-01-01T00:00:00Z\t1\nnewest-success\tsuccess\t2026-01-02T00:00:00Z\t2\ncurrent-in-progress\tin_progress\t2026-01-03T00:00:00Z\t3\nfailed-sha\tfailure\t2026-01-04T00:00:00Z\t4\n';
  } });
  assert.ok(argsSeen.includes('--paginate'));
  assert.ok(argsSeen.some((arg) => arg.includes('daily.yml/runs?branch=main&status=success&per_page=100')));
  assert.deepEqual(runs, [
    { head_sha: 'newest-success', conclusion: 'success', created_at: '2026-01-02T00:00:00Z', run_number: 2 },
    { head_sha: 'older-success', conclusion: 'success', created_at: '2026-01-01T00:00:00Z', run_number: 1 },
  ]);
});

test('manual CI requests a full run', () => {
  assert.deepEqual(selectBaseline({ mode: 'manual', head: 'manual-sha' }), {
    base: '', head: 'manual-sha', full: true, hasChanges: true,
  });
});
