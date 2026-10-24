import { pathToFileURL } from 'node:url';
import { evaluateDailyGate } from './daily-gate-core.mjs';
export { evaluateDailyGate } from './daily-gate-core.mjs';

function main() {
  const report = evaluateDailyGate({
    hasChanges: process.env.HAS_CHANGES === 'true',
    flags: JSON.parse(process.env.NATIVE_FLAGS || '{}') ?? {},
    results: JSON.parse(process.env.JOB_RESULTS || '{}'),
  });
  if (!report.success) {
    console.error(`Daily CI gate failed: ${report.failures.join('; ')}`);
    process.exitCode = 1;
    return;
  }
  console.log('Daily CI gate passed.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
