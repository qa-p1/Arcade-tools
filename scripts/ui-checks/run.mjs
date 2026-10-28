// `npm run ui-checks`: every scenario against the built UI (dist/), as CI runs
// them. Exits non-zero if any check fails or the page logs an error.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';

if (!existsSync('dist/index.html')) {
  console.error('Build the UI first: npm run build');
  process.exit(2);
}
let failed = false;
for (const scenario of ['races', 'ux', 'layout']) {
  const run = spawnSync(process.execPath, ['scripts/ui-checks/harness.mjs', 'dist', `scripts/ui-checks/${scenario}.mjs`], { stdio: 'inherit' });
  failed ||= run.status !== 0;
}
process.exit(failed ? 1 : 0);
