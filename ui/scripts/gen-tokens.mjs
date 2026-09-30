// `npm run tokens`: regenerates ui/design/tokens.css from ui/design/tokens.json.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderTokensCss } from './tokens.mjs';

const json = fileURLToPath(new URL('../design/tokens.json', import.meta.url));
const css = fileURLToPath(new URL('../design/tokens.css', import.meta.url));
writeFileSync(css, renderTokensCss(JSON.parse(readFileSync(json, 'utf8'))));
console.log('Wrote ui/design/tokens.css from tokens.json.');
