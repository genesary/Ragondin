import { API_BASE } from './base.ts';
import type { Paths, Problem } from './types.ts';

/**
 * The header every response carries the answering build's identity in —
 * `BUILD_HEADER` in runtime/ragondin-api. The description declares no header,
 * so this one name is written here rather than generated.
 */
export const BUILD_HEADER = 'x-ragondin-build';

/**
 * The codes the client itself reports, for failures that never reached the
 * API's own error handling: no answer at all, an answer it cannot read, and a
 * different build answering.
 */
export type ClientCode = 'network_failed' | 'response_unreadable' | 'build_mismatch';

/** Every failure a screen renders, whether the API or the client reported it. */
export type ApiProblem = {
  code: Problem['code'] | ClientCode;
  /** What happened, in this occurrence's words. */
  message: string;
  /** The action that would resolve it. */
  hint: string;
  /** Where in a pipeline a validation failure is; null otherwise. */
  location: NonNullable<Problem['location']> | null;
  /** The HTTP status, or null when no answer arrived. */
  status: number | null;
};

export type ApiResult<T> = { ok: true; value: T } | { ok: false; problem: ApiProblem };

export type Method = 'get' | 'post' | 'put' | 'patch' | 'delete';
export type PathWith<M extends Method> = { [P in keyof Paths]: M extends keyof Paths[P] ? P : never }[keyof Paths];
type Operation<P extends keyof Paths, M extends Method> = Paths[P] extends Record<M, infer O> ? O : never;
export type Answer<P extends keyof Paths, M extends Method> = Operation<P, M> extends { response: infer R } ? R : never;
type Body<P extends keyof Paths, M extends Method> = Operation<P, M> extends { body: infer B } ? B : never;
/** No argument for a path without parameters; the parameters otherwise. */
type ParamsArg<P extends keyof Paths, M extends Method> = Operation<P, M> extends { params: infer Q }
  ? Q extends Record<string, never>
    ? []
    : [params: Q]
  : never;

export type ApiClient = {
  get<P extends PathWith<'get'>>(path: P, ...params: ParamsArg<P, 'get'>): Promise<ApiResult<Answer<P, 'get'>>>;
  post<P extends PathWith<'post'>>(path: P, body: Body<P, 'post'>, ...params: ParamsArg<P, 'post'>): Promise<ApiResult<Answer<P, 'post'>>>;
  put<P extends PathWith<'put'>>(path: P, body: Body<P, 'put'>, ...params: ParamsArg<P, 'put'>): Promise<ApiResult<Answer<P, 'put'>>>;
  patch<P extends PathWith<'patch'>>(path: P, body: Body<P, 'patch'>, ...params: ParamsArg<P, 'patch'>): Promise<ApiResult<Answer<P, 'patch'>>>;
  del<P extends PathWith<'delete'>>(path: P, ...params: ParamsArg<P, 'delete'>): Promise<ApiResult<Answer<P, 'delete'>>>;
  /** The build identity the last answer carried; null before any answer, or when the last carried none. */
  build(): string | null;
};

/** A path template of the description, its `{name}` segments filled and encoded. */
function fill(template: string, params: Record<string, string> | undefined): string {
  return template.replace(/\{([^}]+)\}/g, (_, name: string) => encodeURIComponent(params?.[name] ?? ''));
}

const unreadable = (request: string, status: number, why: string): ApiProblem => ({
  code: 'response_unreadable',
  message: `${request} answered ${status}, and ${why}.`,
  hint: 'This build of the UI and the server disagree on the API. Reload the page; if it persists, report it with this message.',
  location: null,
  status,
});

/**
 * The one client over the API. Every request goes to the relative base
 * address, so it stays on the origin that served the page (ARCHITECTURE.md
 * § The one-address rule), and every outcome is a value: a failure is an
 * `ApiProblem` the caller must render, never an exception it could forget.
 */
export function createApiClient(): ApiClient {
  let lastBuild: string | null = null;

  async function request(method: Method, template: string, body: unknown, params: Record<string, string> | undefined): Promise<ApiResult<never>> {
    const url = `${API_BASE}${fill(template, params)}`;
    const name = `${method.toUpperCase()} ${url}`;
    const init: RequestInit = { method: method.toUpperCase(), headers: { accept: 'application/json, application/problem+json' } };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
      init.headers = { ...init.headers, 'content-type': 'application/json' };
    }

    let response: Response;
    try {
      response = await fetch(url, init);
    } catch (e) {
      return {
        ok: false,
        problem: {
          code: 'network_failed',
          message: `${name} failed before any answer: ${e instanceof Error ? e.message : String(e)}.`,
          hint: 'Check that `ragondin ui` is still running and reachable, then retry.',
          location: null,
          status: null,
        },
      };
    }

    // Every answer replaces the identity, an absent header included: the
    // handshake must see "no identity", never an earlier answer's.
    lastBuild = response.headers.get(BUILD_HEADER);
    const type = response.headers.get('content-type') ?? '';
    const text = await response.text();

    if (type.startsWith('application/problem+json')) {
      let problem: Problem;
      try {
        problem = JSON.parse(text) as Problem;
      } catch {
        return { ok: false, problem: unreadable(name, response.status, 'its problem body is not JSON') };
      }
      return {
        ok: false,
        problem: { code: problem.code, message: problem.detail, hint: problem.hint, location: problem.location ?? null, status: response.status },
      };
    }
    if (!response.ok) return { ok: false, problem: unreadable(name, response.status, `its body is ${type === '' ? 'untyped' : type}, not a problem`) };
    if (text === '') return { ok: true, value: null as never };
    try {
      return { ok: true, value: JSON.parse(text) as never };
    } catch {
      return { ok: false, problem: unreadable(name, response.status, 'its body is not JSON') };
    }
  }

  const params = (rest: unknown[]) => rest[0] as Record<string, string> | undefined;
  return {
    get: (path, ...rest) => request('get', path, undefined, params(rest)),
    post: (path, body, ...rest) => request('post', path, body, params(rest)),
    put: (path, body, ...rest) => request('put', path, body, params(rest)),
    patch: (path, body, ...rest) => request('patch', path, body, params(rest)),
    del: (path, ...rest) => request('delete', path, undefined, params(rest)),
    build: () => lastBuild,
  };
}
