// `npm run types:check`, part of `npm run check`: fails when ui/src/api/types.ts
// is not what the API's golden description generates — a description changed
// without regenerating, or a hand edit (ADR-C36 § 2). It regenerates into a
// temporary file and compares it with the committed one, so it writes nothing
// under ui/. Arguments, for its own tests: [description] [types].
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderApiTypes } from './api-types.mjs';

const [descriptionArg, typesArg] = process.argv.slice(2);
const description = descriptionArg ?? fileURLToPath(new URL('../../runtime/ragondin-api/api/v1.json', import.meta.url));
const types = typesArg ?? fileURLToPath(new URL('../src/api/types.ts', import.meta.url));

const fresh = join(mkdtempSync(join(tmpdir(), 'ragondin-types-')), 'types.ts');
writeFileSync(fresh, renderApiTypes(JSON.parse(readFileSync(description, 'utf8'))));

const expected = readFileSync(fresh, 'utf8').split('\n');
const committed = readFileSync(types, 'utf8').split('\n');
const line = expected.findIndex((l, i) => l !== committed[i]);
const differs = line !== -1 || expected.length !== committed.length;

if (differs) {
  const at = line === -1 ? Math.min(expected.length, committed.length) : line;
  console.error(`${types} is stale: it is not what ${description} generates.`);
  console.error(`First difference at line ${at + 1}:`);
  console.error(`  committed: ${committed[at] ?? '(end of file)'}`);
  console.error(`  generated: ${expected[at] ?? '(end of file)'}`);
  console.error(`The generated file is ${fresh}. Run \`just gen-ui-types\` and commit the result; never edit types.ts by hand.`);
  process.exit(1);
}
console.log('ui/src/api/types.ts is current.');
