import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';
import tseslint from 'typescript-eslint';

// The browser's network primitives. Outside `src/api/` no code may name them:
// ADR-012 on the browser side, as ADR-C36 § 5 makes it mechanical. This is the
// second, best-effort layer; the content security policy the binary sends is
// the one that holds. ARCHITECTURE.md § The network lint lists what this scan
// does not see.
const NETWORK_PRIMITIVES = ['fetch', 'XMLHttpRequest', 'WebSocket', 'EventSource'];
const GLOBAL_OBJECTS = ['window', 'globalThis', 'self'];
const message =
  'Network access is confined to src/api/ (ui/ARCHITECTURE.md § The network lint): the UI reaches its own origin only, through that module.';

export default defineConfig([
  globalIgnores(['dist/', 'node_modules/']),
  {
    files: ['**/*.{js,mjs,ts,tsx}'],
    extends: [js.configs.recommended, tseslint.configs.recommended],
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: { globals: globals.browser },
  },
  {
    files: ['*.{js,ts}', 'scripts/**/*.mjs', 'tests/**/*.ts'],
    languageOptions: { globals: globals.node },
  },
  {
    files: ['**/*.{js,mjs,ts,tsx}'],
    ignores: ['src/api/**'],
    rules: {
      'no-restricted-globals': ['error', ...NETWORK_PRIMITIVES.map((name) => ({ name, message }))],
      'no-restricted-properties': [
        'error',
        ...GLOBAL_OBJECTS.flatMap((object) => NETWORK_PRIMITIVES.map((property) => ({ object, property, message }))),
      ],
    },
  },
]);
