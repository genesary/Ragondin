// Prints the steps of `npm run check` other than `npm run build`, joined as the
// script joins them, for `just check-ui` to run: `just build-ui`, which it
// depends on, has already built `dist/`, so `just check` builds once. Read from
// package.json rather than named in the justfile, so a step added to the check
// runs there without an edit. Fails if the build is no longer a step, rather
// than let the recipe build a second time. Argument, for its own tests:
// [package.json].
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const manifest = process.argv[2] ?? fileURLToPath(new URL('../package.json', import.meta.url));
/** @type {string[]} */
const steps = JSON.parse(readFileSync(manifest, 'utf8')).scripts.check.split(' && ');

if (!steps.includes('npm run build')) {
  console.error(`the check script of ${manifest} no longer runs npm run build as a step; update check-ui in the justfile`);
  process.exit(1);
}
console.log(steps.filter((step) => step !== 'npm run build').join(' && '));
