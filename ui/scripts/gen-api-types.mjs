// `just gen-ui-types`: regenerates ui/src/api/types.ts from the API's golden
// description, runtime/ragondin-api/api/v1.json (ADR-C36 § 2).
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderApiTypes } from './api-types.mjs';

const description = fileURLToPath(new URL('../../runtime/ragondin-api/api/v1.json', import.meta.url));
const types = fileURLToPath(new URL('../src/api/types.ts', import.meta.url));
writeFileSync(types, renderApiTypes(JSON.parse(readFileSync(description, 'utf8'))));
console.log('Wrote ui/src/api/types.ts from runtime/ragondin-api/api/v1.json.');
