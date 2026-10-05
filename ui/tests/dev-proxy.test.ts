// The dev server's fixture mode (ARCHITECTURE.md § The end-to-end journeys):
// `npm run dev` hands `/api` to the real binary when RAGONDIN_API names it, so
// the page talks to the fixture workspace through the dev server's own origin
// and still reaches one address (ARCHITECTURE.md § The one-address rule).
import { describe, expect, it } from 'vitest';
import { devProxy } from '../scripts/dev-proxy.mjs';

describe('the dev server proxy', () => {
  it('is absent unless RAGONDIN_API names a server', () => {
    expect(devProxy(undefined)).toBeUndefined();
    expect(devProxy('')).toBeUndefined();
  });

  it('hands /api to the binary as if the page were served by it', () => {
    const proxy = devProxy('http://127.0.0.1:7561/');
    expect(proxy).toEqual({
      '/api': {
        target: 'http://127.0.0.1:7561',
        // The binary answers only its own Host, and refuses a state-changing
        // request from another Origin: both are rewritten to its own.
        changeOrigin: true,
        headers: { origin: 'http://127.0.0.1:7561' },
      },
    });
  });

  it('refuses an address that is not loopback http', () => {
    expect(() => devProxy('https://127.0.0.1:7561')).toThrow(/http:\/\/127\.0\.0\.1/);
    expect(() => devProxy('http://example.com:7561')).toThrow(/loopback/);
  });
});
