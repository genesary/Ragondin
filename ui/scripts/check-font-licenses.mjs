// `npm run audit`, font half: fails on any font file under ui/design/fonts/
// that LICENSES.md does not declare under a licence on the allow list in
// audit-policy.mjs, whose licence text is missing, or that no longer matches
// its recorded digest. Offline: it reads files only.
import { fileURLToPath } from 'node:url';
import { LICENSE_ALLOW } from './audit-policy.mjs';
import { auditFonts } from './font-licenses.mjs';

const fonts = fileURLToPath(new URL('../design/fonts/', import.meta.url));
const problems = auditFonts(fonts, new Set(LICENSE_ALLOW));

if (problems.length > 0) {
  console.error("Font licence audit failed. The allow list is deny.toml's; see ui/ARCHITECTURE.md § The dependency audit.");
  for (const p of problems) console.error(`  ui/design/fonts/${p.file}: ${p.problem}`);
  process.exit(1);
}
console.log('Every font file is declared in ui/design/fonts/LICENSES.md under a licence on the allow list.');
