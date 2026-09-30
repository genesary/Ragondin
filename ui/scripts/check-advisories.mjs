// `npm run audit`, advisory half: `npm audit --audit-level=high`, with the
// exceptions in audit-policy.mjs let through. npm's own exit code cannot honour
// an exception, so the JSON report decides instead of it.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { blockingAdvisories, unmatchedExceptions, validateAdvisoryExceptions } from './advisories.mjs';
import { ADVISORY_EXCEPTIONS } from './audit-policy.mjs';

validateAdvisoryExceptions(ADVISORY_EXCEPTIONS);

const cwd = fileURLToPath(new URL('..', import.meta.url));
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const run = spawnSync(npm, ['audit', '--audit-level=high', '--json'], { cwd, encoding: 'utf8' });
if (run.error !== undefined) throw run.error;

/** @type {unknown} */
let report;
try {
  report = JSON.parse(run.stdout);
} catch {
  // No report at all (the registry unreachable, say) is a failure, never a pass.
  console.error(`npm audit produced no report (exit ${String(run.status)}):\n${run.stderr}`);
  process.exit(1);
}
if (typeof report === 'object' && report !== null && 'error' in report) {
  console.error(`npm audit failed: ${JSON.stringify(report.error)}`);
  process.exit(1);
}

for (const e of unmatchedExceptions(report, ADVISORY_EXCEPTIONS)) {
  console.warn(`warning: advisory exception ${e.id} matches nothing any more; remove it (${e.removedBy}).`);
}

const blocking = blockingAdvisories(report, ADVISORY_EXCEPTIONS);
if (blocking.length > 0) {
  console.error('Advisory audit failed: high or critical advisories no exception names.');
  for (const a of blocking) console.error(`  ${a.id} [${a.severity}] ${a.package}: ${a.title} ${a.url}`);
  process.exit(1);
}
console.log('No high or critical advisory outside the exceptions.');
