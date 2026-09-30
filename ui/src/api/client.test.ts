import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from './client.ts';
import type { Problem, Workspace } from './types.ts';

const WORKSPACE: Workspace = {
  path: '/home/ada/ws',
  build: '0.0.0+0123456789ab',
  settings: { datasets: '/home/ada/ws/datasets', services: [] },
  capabilities: { families: [], remote: false },
};

const json = (body: unknown, init: { status?: number; type?: string; build?: string | null } = {}) => {
  const headers = new Headers({ 'content-type': init.type ?? 'application/json' });
  if (init.build !== null) headers.set('x-ragondin-build', init.build ?? '0.0.0+0123456789ab');
  return new Response(JSON.stringify(body), { status: init.status ?? 200, headers });
};

const stubFetch = (impl: (url: string, init?: RequestInit) => Promise<Response>) => {
  const spy = vi.fn(impl);
  vi.stubGlobal('fetch', spy);
  return spy;
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the API client, on success', () => {
  it('asks the relative base address and returns the body as the generated type', async () => {
    const spy = stubFetch(async () => json(WORKSPACE));
    const result = await createApiClient().get('/workspace');
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/workspace');
    expect(spy.mock.calls[0]?.[1]?.method).toBe('GET');
    expect(result).toEqual({ ok: true, value: WORKSPACE });
  });

  it('fills a path parameter, encoded', async () => {
    const spy = stubFetch(async () => json({}));
    await createApiClient().get('/runs/{id}', { id: 'a/b c' });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/a%2Fb%20c');
  });

  it('keeps the build identity of the last response, and none before one', async () => {
    stubFetch(async () => json(WORKSPACE, { build: '9.9.9+fedcba987654' }));
    const client = createApiClient();
    expect(client.build()).toBeNull();
    await client.get('/workspace');
    expect(client.build()).toBe('9.9.9+fedcba987654');
  });
});

describe('the API client, on a problem', () => {
  const problem: Problem = {
    type: 'urn:ragondin:problem:run_not_found',
    title: 'Run not found',
    status: 404,
    detail: 'No run 1234 in this workspace.',
    code: 'run_not_found',
    hint: 'Open Runs to see the runs this workspace holds.',
  };

  it('parses application/problem+json into an ApiProblem: code, message, hint and location', async () => {
    stubFetch(async () => json(problem, { status: 404, type: 'application/problem+json' }));
    const result = await createApiClient().get('/runs/{id}', { id: '1234' });
    expect(result).toEqual({
      ok: false,
      problem: {
        code: 'run_not_found',
        message: 'No run 1234 in this workspace.',
        hint: 'Open Runs to see the runs this workspace holds.',
        location: null,
        status: 404,
      },
    });
  });

  it('carries a validation failure’s location', async () => {
    const location = { node: 'rerank', edge: null };
    stubFetch(async () => json({ ...problem, code: 'pipeline_invalid', status: 422, location }, { status: 422, type: 'application/problem+json' }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.location).toEqual(location);
  });

  it('keeps the build identity a problem response carries too', async () => {
    stubFetch(async () => json(problem, { status: 404, type: 'application/problem+json', build: '1.0.0+aaaaaaaaaaaa' }));
    const client = createApiClient();
    await client.get('/workspace');
    expect(client.build()).toBe('1.0.0+aaaaaaaaaaaa');
  });

  it('reports an error status without a problem body as unreadable, naming the request and the status', async () => {
    stubFetch(async () => new Response('<html>bad gateway</html>', { status: 502, headers: { 'content-type': 'text/html' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.problem.code).toBe('response_unreadable');
    expect(result.problem.status).toBe(502);
    expect(result.problem.message).toMatch(/GET \/api\/v1\/workspace/);
    expect(result.problem.message).toMatch(/502/);
  });

  it('reports a success whose body is not JSON as unreadable', async () => {
    stubFetch(async () => new Response('not json', { status: 200, headers: { 'content-type': 'application/json' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('response_unreadable');
  });
});

describe('the API client, on a network failure', () => {
  it('returns a network_failed problem naming the request, never throws', async () => {
    stubFetch(async () => {
      throw new TypeError('Failed to fetch');
    });
    const result = await createApiClient().get('/workspace');
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.problem.code).toBe('network_failed');
    expect(result.problem.status).toBeNull();
    expect(result.problem.message).toMatch(/GET \/api\/v1\/workspace/);
    expect(result.problem.message).toMatch(/Failed to fetch/);
    expect(result.problem.hint).toMatch(/ragondin ui/);
  });
});

describe('the API client, writing', () => {
  // No path of the description takes a write yet, so the typed signatures
  // admit none; the cast reaches the one request builder they all share.
  it.each([
    ['post', 'POST'],
    ['put', 'PUT'],
    ['patch', 'PATCH'],
  ] as const)('%s sends its body as JSON', async (method, verb) => {
    const spy = stubFetch(async () => json({ job_id: 'j1' }, { status: 202 }));
    const client = createApiClient();
    const result = await client[method]('/runs' as never, { pipeline: 'p' } as never);
    const init = spy.mock.calls[0]?.[1];
    expect(init?.method).toBe(verb);
    expect(init?.body).toBe('{"pipeline":"p"}');
    expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
    expect(result).toEqual({ ok: true, value: { job_id: 'j1' } });
  });

  it('del sends DELETE and reads an empty answer as null', async () => {
    const spy = stubFetch(async () => new Response(null, { status: 204, headers: { 'x-ragondin-build': 'b' } }));
    const del = createApiClient().del as (path: string, params: Record<string, string>) => Promise<unknown>;
    const result = await del('/runs/{id}', { id: 'j1' });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/j1');
    expect(spy.mock.calls[0]?.[1]?.method).toBe('DELETE');
    expect(result).toEqual({ ok: true, value: null });
  });
});
