import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { planNative } from './native-plan-core.mjs';
export { planNative } from './native-plan-core.mjs';

function main() {
  const [projectsPath] = process.argv.slice(2);
  const full = process.env.NX_FULL === 'true';
  if (!projectsPath && !full) throw new Error('Pass the JSON file produced by `nx show projects --affected --withTarget=native --json`.');
  const projects = full ? [] : JSON.parse(readFileSync(projectsPath, 'utf8'));
  if (!Array.isArray(projects) || projects.some((project) => typeof project !== 'string')) {
    throw new Error('Nx affected project output must be a JSON array of names.');
  }
  const plan = planNative({ projects, full });
  appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(plan).map(([key, value]) => `${key}=${value}`).join('\n') + '\n');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
