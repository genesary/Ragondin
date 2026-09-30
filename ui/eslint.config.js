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
// Every name the browser binds to a window or the global object. Forms that
// still pass are listed in ARCHITECTURE.md § The network lint.
const GLOBAL_OBJECTS = ['window', 'globalThis', 'self', 'top', 'parent', 'frames', 'opener'];
// Every extension tsc and Vite accept. A file the lint does not match is a file
// it does not read, so a narrower list would be a silent way around the rule.
const SOURCES = '**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}';
// src/api/testing.ts, named through any path that reaches it from outside src/api/.
const TESTING_FROM_ANYWHERE = '(^|/)api/testing(\\.[cm]?[jt]sx?)?$';
const testingMessage =
  'src/api/testing.ts holds test doubles; only tests import it, so none of it reaches the bundle (ui/ARCHITECTURE.md § The client).';
const message =
  'Network access is confined to src/api/ (ui/ARCHITECTURE.md § The network lint): the UI reaches its own origin only, through that module.';

export default defineConfig([
  globalIgnores(['dist/', 'node_modules/']),
  {
    files: [SOURCES],
    extends: [js.configs.recommended, tseslint.configs.recommended],
  },
  {
    files: ['src/**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}', 'design/**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}'],
    languageOptions: { globals: globals.browser },
  },
  {
    files: ['*.{js,mjs,cjs,ts,mts,cts}', 'scripts/**/*.{js,mjs,cjs,ts,mts,cts}', 'tests/**/*.{js,mjs,cjs,ts,mts,cts}'],
    languageOptions: { globals: globals.node },
  },
  {
    files: [SOURCES],
    ignores: ['src/api/**'],
    rules: {
      'no-restricted-globals': ['error', ...NETWORK_PRIMITIVES.map((name) => ({ name, message }))],
      'no-restricted-properties': [
        'error',
        ...GLOBAL_OBJECTS.flatMap((object) => NETWORK_PRIMITIVES.map((property) => ({ object, property, message }))),
      ],
    },
  },
  {
    // The network's test doubles stay out of the bundle: only a test may
    // import them. ARCHITECTURE.md § The client; tests/test-doubles.test.ts.
    files: [SOURCES],
    ignores: ['**/*.test.*', 'tests/**'],
    rules: { 'no-restricted-imports': ['error', { patterns: [{ regex: TESTING_FROM_ANYWHERE, message: testingMessage }] }] },
  },
  {
    // Inside src/api/ the module is a sibling (`./testing`) or a parent's
    // (`../testing`), which the path above does not name. A later block
    // replaces the rule's options rather than adding to them, so both
    // patterns are listed.
    files: ['src/api/**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}'],
    ignores: ['**/*.test.*'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            { regex: TESTING_FROM_ANYWHERE, message: testingMessage },
            { regex: '^(\\./|(\\.\\./)+)testing(\\.[cm]?[jt]sx?)?$', message: testingMessage },
          ],
        },
      ],
    },
  },
]);
