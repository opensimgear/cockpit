import { execFileSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { selectBaselineWithFallback } from './baseline-core.mjs';
export { selectBaseline, selectBaselineWithFallback } from './baseline-core.mjs';

export function readSuccessfulRuns({ repo, workflow, branch, execute = execFileSync }) {
  const endpoint = `repos/${repo}/actions/workflows/${workflow}/runs?branch=${branch}&status=success&per_page=100`;
  const rows = execute('gh', [
    'api', '--paginate', '--jq', '.workflow_runs[] | [.head_sha, .conclusion, .created_at, .run_number] | @tsv', endpoint,
  ], { encoding: 'utf8' });
  return rows.trim().split(/\r?\n/).filter(Boolean).map((row) => {
    const [head_sha, conclusion, created_at, run_number] = row.split('\t');
    return { head_sha, conclusion, created_at, run_number: Number(run_number) };
  }).filter((run) => run.conclusion === 'success')
    .sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at) || right.run_number - left.run_number);
}

function gitRootCommit() {
  return execFileSync('git', ['rev-list', '--max-parents=0', 'HEAD'], { encoding: 'utf8' }).trim().split(/\s+/)[0];
}

function canCompareCommits(base, head) {
  try {
    execFileSync('git', ['merge-base', '--is-ancestor', base, head], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

function main() {
  const [mode] = process.argv.slice(2);
  const event = process.env.GITHUB_EVENT_PATH ? JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8')) : {};
  const repo = process.env.GITHUB_REPOSITORY;
  const head = process.env.GITHUB_SHA;
  const eventBase = mode === 'pull_request' ? event.pull_request?.base?.sha : undefined;
  const workflow = mode === 'daily' ? 'daily.yml' : 'ci.yml';
  const baseline = selectBaselineWithFallback({
    mode,
    head,
    eventBase,
    readRuns: () => readSuccessfulRuns({ repo, workflow, branch: 'main' }),
    canCompare: canCompareCommits,
    onReadError: (error) => console.warn(`Could not read successful ${workflow} runs; selecting a full run: ${error.message}`),
  });
  if (baseline.full) baseline.base = gitRootCommit();

  appendFileSync(process.env.GITHUB_OUTPUT, [
    `base=${baseline.base}`,
    `head=${baseline.head}`,
    `full=${baseline.full}`,
    `has_changes=${baseline.hasChanges}`,
  ].join('\n') + '\n');
  appendFileSync(process.env.GITHUB_ENV, `NX_BASE=${baseline.base}\nNX_HEAD=${baseline.head}\nNX_FULL=${baseline.full}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
