// `npm run notices`, after `npm run build`: fails when dist/ carries no
// third-party notices file, or when it lacks the notice — name, version,
// licence and the licence text the installed package ships — of a runtime
// package of the lockfile or of a font licence. The build already refuses a
// bundled module no notice covers; this re-reads what it wrote
// (ARCHITECTURE.md § The third-party notices).
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { BUILD_INJECTED, fontNotices, missingNotices, NOTICES_FILE, noticeEntries } from './notices.mjs';

const ui = fileURLToPath(new URL('..', import.meta.url));
const path = join(ui, 'dist', NOTICES_FILE);

if (!existsSync(path)) {
  console.error(`ui/dist/${NOTICES_FILE} is absent: run \`npm run build\` first. See ui/ARCHITECTURE.md § The third-party notices.`);
  process.exit(1);
}
const missing = missingNotices(readFileSync(path, 'utf8'), noticeEntries(ui, BUILD_INJECTED), fontNotices(ui));
if (missing.length > 0) {
  console.error(`ui/dist/${NOTICES_FILE} lacks these notices, or carries a stale copy of them. See ui/ARCHITECTURE.md § The third-party notices.`);
  for (const name of missing) console.error(`  ${name}`);
  process.exit(1);
}
console.log(`ui/dist/${NOTICES_FILE} carries the notice of every runtime package and font licence.`);
