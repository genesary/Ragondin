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

  it('holds an answer until the test resolves it, so answers can arrive out of order', async () => {
    let release: (reply: { body: Comparison }) => void = () => {};
    mockApi({ 'POST /compare': () => new Promise((resolve) => (release = resolve)) });
    const pending = createApiClient().post('/compare', { run_ids: ['a', 'b'], baseline: 'a' });
    let settled = false;
    void pending.then(() => (settled = true));
    await new Promise((r) => setTimeout(r, 0));
    expect(settled).toBe(false);
    release({ body: ANSWER });
    const result = await pending;
    expect(result.ok && result.value).toEqual(ANSWER);
  });

  it('records no body for a GET', async () => {
    const api = mockApi({ 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } } });
    await createApiClient().get('/runs');
    expect(api.bodies).toEqual([undefined]);
  });
});

describe('mockApi, a query string', () => {
  const QUERIES = { run: 'r', answer_node: null, cache_error: null, metrics: [], nodes: [], queries: [], ranking_node: null } as unknown as import('./types.ts').RunQueries;

  it('answers a request carrying a query string from its path template, and records the query string', async () => {
    const api = mockApi({ 'GET /runs/{id}/queries': { body: QUERIES } });
    const result = await createApiClient().get('/runs/{id}/queries', { id: 'r' }, { query: { missing_gold_at: 10 } });
    expect(result.ok).toBe(true);
    expect(api.requests).toEqual(['GET /api/v1/runs/r/queries?missing_gold_at=10']);
  });

  it('lets a GET reply depend on the query string it was sent', async () => {
    mockApi({ 'GET /runs/{id}/queries': (query) => ({ body: { ...QUERIES, run: query.get('missing_gold_at') ?? 'all' } }) });
    const client = createApiClient();
    const all = await client.get('/runs/{id}/queries', { id: 'r' });
    const missing = await client.get('/runs/{id}/queries', { id: 'r' }, { query: { missing_gold_at: 3 } });
    expect(all.ok && all.value.run).toBe('all');
    expect(missing.ok && missing.value.run).toBe('3');
  });

  it('hands a GET reply the path it was sent, so one template can answer two ids', async () => {
    mockApi({ 'GET /runs/{id}/queries': (_query, path) => ({ body: { ...QUERIES, run: path } }) });
    const result = await createApiClient().get('/runs/{id}/queries', { id: 'r 1' }, { query: { missing_gold_at: 3 } });
    expect(result.ok && result.value.run).toBe('/api/v1/runs/r%201/queries');
  });

  it('still refuses a path that only starts like a template', async () => {
    mockApi({ 'GET /runs/{id}': { body: {} as import('./types.ts').RunDetail } });
    const result = await createApiClient().get('/runs/{id}/queries', { id: 'r' });
    expect(!result.ok && result.problem.code).toBe('route_not_found');
  });
});

describe('mockApi, cancelled', () => {
  it('records each request’s signal, and rejects a held answer as fetch does once its signal aborts', async () => {
    const api = mockApi({ 'POST /compare': () => new Promise(() => {}), 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } } });
    const client = createApiClient();
    const controller = new AbortController();
    const pending = client.post('/compare', { run_ids: ['a', 'b'], baseline: 'a' }, { signal: controller.signal });
    await client.get('/runs');
    expect(api.signals).toEqual([controller.signal, undefined]);
    controller.abort();
    const result = await pending;
    expect(result.ok ? null : result.problem.code).toBe('request_aborted');
  });

  it('rejects a request whose signal had already aborted, without answering it', async () => {
    mockApi({ 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } } });
    const controller = new AbortController();
    controller.abort();
    const result = await createApiClient().get('/runs', { signal: controller.signal });
    expect(result.ok ? null : result.problem.code).toBe('request_aborted');
  });
});
