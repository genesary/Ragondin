// `npm run audit`, font half: fails on any font file under ui/ that is not in
// ui/design/fonts/ with a row in its LICENSES.md, under a licence on the allow
// list in audit-policy.mjs, beside a pinned licence text, and unchanged since
// it was committed. Offline: it reads files only.
import { fileURLToPath } from 'node:url';
import { LICENSE_ALLOW } from './audit-policy.mjs';
import { auditFonts } from './font-licenses.mjs';

const ui = fileURLToPath(new URL('..', import.meta.url));
const problems = auditFonts(ui, new Set(LICENSE_ALLOW));

if (problems.length > 0) {
  console.error("Font licence audit failed. The allow list is deny.toml's; see ui/ARCHITECTURE.md § The dependency audit.");
  for (const p of problems) console.error(`  ui/${p.file}: ${p.problem}`);
  process.exit(1);
}
console.log('Every font file is declared in ui/design/fonts/LICENSES.md under a licence on the allow list.');
