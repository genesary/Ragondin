/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from 'vitest';
import { vi } from 'vitest';
import { createApiClient } from './client.ts';
import { mockApi } from './testing.ts';
import type { Comparison } from './types.ts';

afterEach(() => vi.unstubAllGlobals());

const ANSWER = { baseline: 'a' } as Comparison;

describe('mockApi, writing', () => {
  it('answers a POST from its route, records the body sent, and lets the reply depend on it', async () => {
    const api = mockApi({
      'POST /compare': (body) => (body.run_ids.length > 5 ? { problem: { type: 'urn:ragondin:problem:runs_not_comparable', title: 't', status: 409, detail: 'too many', code: 'runs_not_comparable', hint: 'h' } } : { body: ANSWER }),
    });
    const client = createApiClient();
    const ok = await client.post('/compare', { run_ids: ['a', 'b'], baseline: 'a' });
    expect(ok.ok && ok.value).toEqual(ANSWER);
    const refused = await client.post('/compare', { run_ids: ['a', 'b', 'c', 'd', 'e', 'f'], baseline: 'a' });
    expect(!refused.ok && refused.problem.message).toBe('too many');
    expect(api.requests).toEqual(['POST /api/v1/compare', 'POST /api/v1/compare']);
    expect(api.bodies).toEqual([
      { run_ids: ['a', 'b'], baseline: 'a' },
      { run_ids: ['a', 'b', 'c', 'd', 'e', 'f'], baseline: 'a' },
    ]);
  });

  it('records no body for a GET', async () => {
    const api = mockApi({ 'GET /runs': { body: { runs: [], unreadable: [] } } });
    await createApiClient().get('/runs');
    expect(api.bodies).toEqual([undefined]);
  });
});
