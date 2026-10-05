import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import { buildIdentity } from './scripts/build-identity.mjs';
import { devProxy } from './scripts/dev-proxy.mjs';
import { thirdPartyNotices } from './scripts/notices.mjs';

// No React plugin: esbuild compiles JSX with the automatic runtime on its own,
// and the one thing the plugin adds, Fast Refresh in the dev server, does not
// yet pay for the plugin's transitive tree. ARCHITECTURE.md § Toolchain says why.
export default defineConfig({
  esbuild: { jsx: 'automatic' },
  // The build identity, computed from the build rather than read from the
  // first response, or the handshake would compare the server with itself.
  // ARCHITECTURE.md § The build identity handshake.
  define: { __RAGONDIN_BUILD__: JSON.stringify(buildIdentity(fileURLToPath(new URL('..', import.meta.url)))) },
  // The binary redistributes the bundle, so the bundle carries the notices
  // of what it holds. ARCHITECTURE.md § The third-party notices.
  plugins: [thirdPartyNotices(fileURLToPath(new URL('.', import.meta.url)))],
  // The fixture mode: `/api` handed to the binary RAGONDIN_API names, which
  // `npm run dev:fixture` starts. ARCHITECTURE.md § The end-to-end journeys.
  server: { proxy: devProxy(process.env.RAGONDIN_API) },
  test: {
    // The governance tests read files and run ESLint, so the default
    // environment is Node; a component test opts into a DOM with a
    // `@vitest-environment happy-dom` docblock.
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}', 'design/**/*.test.{ts,tsx}', 'tests/**/*.test.ts'],
    setupFiles: ['tests/setup.ts'],
    // Stylesheets are processed, not blanked: a component test reads its own
    // stylesheet with `?raw` to assert the rule that draws a state.
    css: true,
  },
});
