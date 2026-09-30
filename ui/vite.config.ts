import { defineConfig } from 'vitest/config';

// No React plugin: esbuild compiles JSX with the automatic runtime on its own,
// and the one thing the plugin adds, Fast Refresh in the dev server, does not
// yet pay for the plugin's transitive tree. ARCHITECTURE.md § Toolchain says why.
export default defineConfig({
  esbuild: { jsx: 'automatic' },
  test: {
    // The governance tests read files and run ESLint, so the default
    // environment is Node; a component test opts into a DOM with a
    // `@vitest-environment happy-dom` docblock.
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}', 'tests/**/*.test.ts'],
  },
});
