import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from './client.ts';
import type { Problem, Workspace } from './types.ts';

// No operation of the golden description answers empty yet; this file's
// client reads one that does, so the empty-answer rule is exercised both ways.
vi.mock('./types.ts', () => ({ EMPTY_ANSWERS: ['DELETE /runs/{id}'] }));

const WORKSPACE: Workspace = {
  path: '/home/ada/ws',
  build: '0.0.0+0123456789ab',
  settings: { datasets: '/home/ada/ws/datasets', services: [] },
  capabilities: { families: [], remote: false },
  counts: { pipelines: 0, runs: 0, benchmarks_ready: 0, services_connected: 0 },
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
    expect(result).toEqual({ ok: true, value: WORKSPACE, build: '0.0.0+0123456789ab' });
  });

  it('fills a path parameter, encoded', async () => {
    const spy = stubFetch(async () => json({}));
    await createApiClient().get('/runs/{id}', { id: 'a/b c' });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/a%2Fb%20c');
  });

  it('returns the build identity each answer carried, with that answer', async () => {
    stubFetch(async () => json(WORKSPACE, { build: '9.9.9+fedcba987654' }));
    const result = await createApiClient().get('/workspace');
    expect(result.build).toBe('9.9.9+fedcba987654');
  });

  it('returns no identity for an answer that carried none', async () => {
    stubFetch(async () => json(WORKSPACE, { build: null }));
    expect((await createApiClient().get('/workspace')).build).toBeNull();
  });

  it('gives each of two concurrent requests its own answer’s identity, whatever order they finish in', async () => {
    let releaseFirst = () => {};
    const firstHeld = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let call = 0;
    stubFetch(async () => {
      call += 1;
      if (call === 1) {
        await firstHeld;
        return json(WORKSPACE, { build: '1.0.0+aaaaaaaaaaaa' });
      }
      return json(WORKSPACE, { build: '2.0.0+bbbbbbbbbbbb' });
    });
    const client = createApiClient();
    const first = client.get('/workspace');
    const second = await client.get('/workspace');
    releaseFirst();
    expect(second.build).toBe('2.0.0+bbbbbbbbbbbb');
    expect((await first).build).toBe('1.0.0+aaaaaaaaaaaa');
  });

  it('refuses a path parameter of `.` or `..` before any request, which would name another path', async () => {
    const spy = stubFetch(async () => json({}));
    for (const id of ['.', '..']) {
      const result = await createApiClient().get('/runs/{id}', { id });
      expect(result.ok ? null : result.problem.code).toBe('request_invalid');
      expect(result.ok ? null : result.problem.message).toMatch(/GET \/api\/v1\/runs\/\{id\}/);
    }
    expect(spy).not.toHaveBeenCalled();
  });
});

describe('the API client, with query and header parameters', () => {
  // An untyped door onto the same request builder, for a parameter the
  // description has no operation for.
  type Untyped = (path: string, ...rest: unknown[]) => Promise<unknown>;

  it('serializes a declared query parameter after the filled path, on the relative base address', async () => {
    const spy = stubFetch(async () => json({}));
    await createApiClient().get('/runs/{id}/queries', { id: 'r 1' }, { query: { missing_gold_at: 3 } });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/r%201/queries?missing_gold_at=3');
  });

  it('percent-encodes reserved characters in a name and a value, as URLSearchParams does', async () => {
    const spy = stubFetch(async () => json({}));
    await (createApiClient().get as Untyped)('/runs/{id}/queries', { id: 'r1' }, { query: { 'a&b': 'c=d e/é?#' } });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/r1/queries?a%26b=c%3Dd+e%2F%C3%A9%3F%23');
  });

  it('omits an absent optional parameter, and the `?` when none is left', async () => {
    const spy = stubFetch(async () => json({}));
    const client = createApiClient();
    await client.get('/runs/{id}/queries', { id: 'r1' });
    await client.get('/runs/{id}/queries', { id: 'r1' }, { query: {} });
    await (client.get as Untyped)('/runs/{id}/queries', { id: 'r1' }, { query: { missing_gold_at: undefined } });
    expect(spy.mock.calls.map((call) => call[0])).toEqual([
      '/api/v1/runs/r1/queries',
      '/api/v1/runs/r1/queries',
      '/api/v1/runs/r1/queries',
    ]);
  });

  it('sends a declared header with the request', async () => {
    const spy = stubFetch(async () => json({ name: 'p', etag: 'e2', hash: 'h' }));
    await createApiClient().put('/pipelines/{name}', { document: 'pipeline: {}' }, { name: 'p' }, { headers: { 'If-Match': '"e1"' } });
    const headers = new Headers(spy.mock.calls[0]?.[1]?.headers);
    expect(headers.get('if-match')).toBe('"e1"');
    expect(headers.get('content-type')).toBe('application/json');
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/pipelines/p');
  });

  it('types the options by the description: none where it declares none, and only the declared ones', async () => {
    stubFetch(async () => json({}));
    const client = createApiClient();
    // @ts-expect-error `/workspace` declares no query parameter and no header.
    await client.get('/workspace', { query: { x: 1 } });
    // @ts-expect-error `missing_gold_at` is a number.
    await client.get('/runs/{id}/queries', { id: 'r1' }, { query: { missing_gold_at: 'three' } });
    // @ts-expect-error `/runs/{id}/queries` declares no header.
    await client.get('/runs/{id}/queries', { id: 'r1' }, { headers: { 'If-Match': '*' } });
    // @ts-expect-error a header the write does not declare.
    await client.put('/pipelines/{name}', { document: '' }, { name: 'p' }, { headers: { 'X-Other': '1' } });
  });

  it('posts to an operation that declares no request body with `undefined`, and sends no body', async () => {
    const spy = stubFetch(async () => json({ job_id: '1-0' }, { status: 202 }));
    const result = await createApiClient().post('/benchmarks/{name}/download', undefined, { name: 'beir/scifact' });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/benchmarks/beir%2Fscifact/download');
    expect(spy.mock.calls[0]?.[1]?.body).toBeUndefined();
    expect(new Headers(spy.mock.calls[0]?.[1]?.headers).get('content-type')).toBeNull();
    expect(result).toEqual({ ok: true, value: { job_id: '1-0' }, build: '0.0.0+0123456789ab' });
    // @ts-expect-error the operation declares no request body.
    await createApiClient().post('/benchmarks/{name}/download', { name: 'x' }, { name: 'x' });
  });

  it('sends no header that was not given', async () => {
    const spy = stubFetch(async () => json({ name: 'p', etag: 'e2', hash: 'h' }));
    await createApiClient().put('/pipelines/{name}', { document: 'pipeline: {}' }, { name: 'p' }, { headers: { 'If-None-Match': '*' } });
    const headers = new Headers(spy.mock.calls[0]?.[1]?.headers);
    expect(headers.get('if-none-match')).toBe('*');
    expect(headers.has('if-match')).toBe(false);
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
      build: '0.0.0+0123456789ab',
    });
  });

  it('carries the parameter a parameter_invalid names, and none when it names none', async () => {
    const refused = { ...problem, code: 'parameter_invalid', status: 400 } as const;
    stubFetch(async () => json({ ...refused, name: 'missing_gold_at' }, { status: 400, type: 'application/problem+json' }));
    const named = await createApiClient().get('/workspace');
    expect(named.ok ? null : named.problem.name).toBe('missing_gold_at');
    stubFetch(async () => json(refused, { status: 400, type: 'application/problem+json' }));
    const unnamed = await createApiClient().get('/workspace');
    expect(unnamed.ok ? null : 'name' in unnamed.problem).toBe(false);
  });

  it('carries a validation failure’s location', async () => {
    const location = { node: 'rerank', edge: null };
    stubFetch(async () => json({ ...problem, code: 'pipeline_invalid', status: 422, location }, { status: 422, type: 'application/problem+json' }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.location).toEqual(location);
  });

  it.each([
    ['a string', '"x"'],
    ['a node that is not a string', '{"node":1,"edge":null}'],
    ['an edge that is not an object', '{"node":null,"edge":"e"}'],
    // Both members are required: an absent one is no location either.
    ['an empty object', '{}'],
    ['a node without an edge', '{"node":"x"}'],
    ['an edge without a node', '{"edge":null}'],
  ])('reports a problem whose location is %s as unreadable, never as a location it is not', async (_, location) => {
    const body = `{"code":"pipeline_invalid","detail":"d","hint":"h","location":${location}}`;
    stubFetch(async () => new Response(body, { status: 422, headers: { 'content-type': 'application/problem+json' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('response_unreadable');
  });

  it('keeps the build identity a problem response carries too', async () => {
    stubFetch(async () => json(problem, { status: 404, type: 'application/problem+json', build: '1.0.0+aaaaaaaaaaaa' }));
    expect((await createApiClient().get('/workspace')).build).toBe('1.0.0+aaaaaaaaaaaa');
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

  it.each([
    ['null', 'null'],
    ['an empty object', '{}'],
    ['a code that is not a string', '{"code":1,"detail":"d","hint":"h"}'],
    ['a list', '[]'],
  ])('reports a problem body that is %s as unreadable, never as a problem with missing fields', async (_, body) => {
    stubFetch(async () => new Response(body, { status: 500, headers: { 'content-type': 'application/problem+json' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('response_unreadable');
    expect(result.ok ? null : result.problem.status).toBe(500);
  });

  it('reports a body that fails while being read as a network failure, never throws', async () => {
    const broken = new ReadableStream({
      start(controller) {
        controller.error(new TypeError('connection reset'));
      },
    });
    stubFetch(async () => new Response(broken, { status: 200, headers: { 'content-type': 'application/json' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('network_failed');
    expect(result.ok ? null : result.problem.message).toMatch(/connection reset/);
  });

  it('reports an empty success body as unreadable unless the answer is 204', async () => {
    stubFetch(async () => new Response('', { status: 200, headers: { 'content-type': 'application/json' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('response_unreadable');
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
    expect(result.build).toBeNull();
    expect(result.problem.message).toMatch(/GET \/api\/v1\/workspace/);
    expect(result.problem.message).toMatch(/Failed to fetch/);
    expect(result.problem.hint).toMatch(/ragondin ui/);
  });
});

describe('the API client, cancelled', () => {
  it('passes the signal it is given to fetch, on a path with and without parameters and options', async () => {
    const spy = stubFetch(async () => json({}));
    const client = createApiClient();
    const signal = new AbortController().signal;
    await client.get('/workspace', { signal });
    await client.get('/runs/{id}', { id: 'r1' }, { signal });
    await client.get('/runs/{id}/queries', { id: 'r1' }, { query: { missing_gold_at: 10 }, signal });
    await client.put('/pipelines/{name}', { document: '' }, { name: 'p' }, { headers: { 'If-Match': '"e1"' }, signal });
    expect(spy.mock.calls.map((call) => call[1]?.signal)).toEqual([signal, signal, signal, signal]);
    expect(spy.mock.calls[2]?.[0]).toBe('/api/v1/runs/r1/queries?missing_gold_at=10');
  });

  it('reports a request aborted before its answer as request_aborted, never as a network failure, and never throws', async () => {
    stubFetch(
      (_url, init) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => reject(new DOMException('The operation was aborted.', 'AbortError')));
        }),
    );
    const controller = new AbortController();
    const pending = createApiClient().get('/runs/{id}', { id: 'r1' }, { signal: controller.signal });
    controller.abort();
    const result = await pending;
    expect(result.ok ? null : result.problem.code).toBe('request_aborted');
    expect(result.build).toBeNull();
  });

  it('reports a request aborted while its body is read as request_aborted', async () => {
    const controller = new AbortController();
    const body = new ReadableStream({
      start(stream) {
        controller.signal.addEventListener('abort', () => stream.error(new DOMException('The operation was aborted.', 'AbortError')));
      },
    });
    stubFetch(async () => new Response(body, { status: 200, headers: { 'content-type': 'application/json' } }));
    const pending = createApiClient().get('/workspace', { signal: controller.signal });
    await new Promise((r) => setTimeout(r, 0));
    controller.abort();
    const result = await pending;
    expect(result.ok ? null : result.problem.code).toBe('request_aborted');
  });
});

describe('the API client, writing', () => {
  // `/runs` takes no write, so no typed signature admits this call; the cast
  // reaches the one request builder the three methods share.
  it.each([
    ['post', 'POST'],
    ['put', 'PUT'],
    ['patch', 'PATCH'],
  ] as const)('%s sends its body as JSON', async (method, verb) => {
    const spy = stubFetch(async () => json({ job_id: 'j1' }, { status: 202 }));
    const client = createApiClient();
    const write = client[method] as (path: string, body: unknown) => Promise<unknown>;
    const result = await write('/runs', { pipeline: 'p' });
    const init = spy.mock.calls[0]?.[1];
    expect(init?.method).toBe(verb);
    expect(init?.body).toBe('{"pipeline":"p"}');
    expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
    expect(result).toEqual({ ok: true, value: { job_id: 'j1' }, build: '0.0.0+0123456789ab' });
  });

  it('reports a 204 from an operation that declares a body as unreadable, never as a null of its type', async () => {
    stubFetch(async () => new Response(null, { status: 204, headers: { 'x-ragondin-build': 'b' } }));
    const result = await createApiClient().get('/workspace');
    expect(result.ok ? null : result.problem.code).toBe('response_unreadable');
  });

  it('del sends DELETE and reads an empty answer as null', async () => {
    const spy = stubFetch(async () => new Response(null, { status: 204, headers: { 'x-ragondin-build': 'b' } }));
    const del = createApiClient().del as (path: string, params: Record<string, string>) => Promise<unknown>;
    const result = await del('/runs/{id}', { id: 'j1' });
    expect(spy.mock.calls[0]?.[0]).toBe('/api/v1/runs/j1');
    expect(spy.mock.calls[0]?.[1]?.method).toBe('DELETE');
    expect(result).toEqual({ ok: true, value: null, build: 'b' });
  });
});
