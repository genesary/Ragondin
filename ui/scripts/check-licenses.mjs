// `npm run audit`, licence half: fails on any package in the tree, runtime or
// development, whose licence is not on the allow list in audit-policy.mjs, or
// that has none.
import { fileURLToPath } from 'node:url';
import { LICENSE_ALLOW } from './audit-policy.mjs';
import { auditLicenses } from './licenses.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const problems = auditLicenses(root, { allow: LICENSE_ALLOW });

if (problems.length > 0) {
  console.error('Licence audit failed. The allow list is deny.toml\'s; see ui/ARCHITECTURE.md § The dependency audit.');
  for (const p of problems) {
    console.error(`  ${p.package}@${p.version}: ${p.problem}${p.license === null ? '' : ` (${p.license})`}`);
  }
  process.exit(1);
}
console.log('Every licence in the npm tree is on the allow list.');
