import { describe, expect, it } from 'vitest';
import { API_BASE } from './base.ts';

// ADR-012 on the browser side: the UI reaches its own origin and nothing else.
// A relative base address is what keeps every request built from it there.
describe('API_BASE', () => {
  it('is the versioned API prefix', () => {
    expect(API_BASE).toBe('/api/v1');
  });

  it('is a relative path that resolves against whatever origin served the page', () => {
    // Neither a scheme nor a protocol-relative `//host` prefix: both would name
    // another origin.
    expect(API_BASE).not.toMatch(/^[a-z][a-z0-9+.-]*:/i);
    expect(API_BASE.startsWith('//')).toBe(false);
    expect(API_BASE.startsWith('/')).toBe(true);

    for (const origin of ['http://127.0.0.1:8080', 'http://localhost:4173']) {
      expect(new URL(API_BASE, origin).origin).toBe(origin);
    }
  });
});
