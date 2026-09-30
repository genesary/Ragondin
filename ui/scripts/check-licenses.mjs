// `npm run audit`, licence half: fails on any package in the tree whose
// licence the policy in audit-policy.mjs does not admit, or that has none.
import { fileURLToPath } from 'node:url';
import { LICENSE_ALLOW, LICENSE_EXCEPTIONS } from './audit-policy.mjs';
import { auditLicenses, unmatchedLicenseExceptions } from './licenses.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const policy = { allow: LICENSE_ALLOW, exceptions: LICENSE_EXCEPTIONS };

for (const e of unmatchedLicenseExceptions(root, policy)) {
  console.warn(`warning: licence exception ${e.package} (${e.license}) admits nothing any more; remove it.`);
}

const problems = auditLicenses(root, policy);

if (problems.length > 0) {
  console.error('Licence audit failed. The allow list is deny.toml\'s; see ui/ARCHITECTURE.md § The dependency audit.');
  for (const p of problems) {
    console.error(`  ${p.package}@${p.version}: ${p.problem}${p.license === null ? '' : ` (${p.license})`}`);
  }
  process.exit(1);
}
console.log('Every licence in the npm tree is admitted by the policy.');
