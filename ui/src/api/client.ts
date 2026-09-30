import { API_BASE } from './base.ts';
import { EMPTY_ANSWERS, type Paths, type Problem } from './types.ts';

/**
 * The header every response carries the answering build's identity in —
 * `BUILD_HEADER` in runtime/ragondin-api. The description declares no header,
 * so this one name is written here rather than generated.
 */
export const BUILD_HEADER = 'x-ragondin-build';

/**
 * The codes the client itself reports, for failures that never reached the
 * API's own error handling: no answer at all, an answer it cannot read, a
 * different build answering, and a request it refused to send.
 */
export type ClientCode = 'network_failed' | 'response_unreadable' | 'build_mismatch' | 'request_invalid';

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

/**
 * A request's outcome, with the build identity its answer carried — null when
 * no answer arrived, or when it carried none. The identity travels with its
 * answer rather than living on the client, so two requests in flight never
 * report each other's.
 */
export type ApiResult<T> = ({ ok: true; value: T } | { ok: false; problem: ApiProblem }) & { build: string | null };

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
};

/**
 * A path template of the description, its `{name}` segments filled and
 * encoded; null when a value is `.` or `..`, which encoding leaves as it is
 * and a URL resolves as another path.
 */
function fill(template: string, params: Record<string, string> | undefined): string | null {
  let refused = false;
  const path = template.replace(/\{([^}]+)\}/g, (_, name: string) => {
    const value = params?.[name] ?? '';
    if (value === '.' || value === '..') refused = true;
    return encodeURIComponent(value);
  });
  return refused ? null : path;
}

/** Whether a parsed problem body has the members every problem carries. */
const isProblem = (value: unknown): value is Problem =>
  typeof value === 'object' &&
  value !== null &&
  !Array.isArray(value) &&
  typeof (value as Problem).code === 'string' &&
  typeof (value as Problem).detail === 'string' &&
  typeof (value as Problem).hint === 'string';

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
  async function request(method: Method, template: string, body: unknown, params: Record<string, string> | undefined): Promise<ApiResult<never>> {
    const verb = method.toUpperCase();
    const path = fill(template, params);
    if (path === null) {
      return {
        ok: false,
        build: null,
        problem: {
          code: 'request_invalid',
          message: `${verb} ${API_BASE}${template} was not sent: a path parameter is \`.\` or \`..\`, which would name another path.`,
          hint: 'Check the address this view was opened from; an id is never `.` or `..`.',
          location: null,
          status: null,
        },
      };
    }
    const url = `${API_BASE}${path}`;
    const name = `${verb} ${url}`;
    const init: RequestInit = { method: verb, headers: { accept: 'application/json, application/problem+json' } };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
      init.headers = { ...init.headers, 'content-type': 'application/json' };
    }
    const networkFailed = (e: unknown, status: number | null, what: string): ApiResult<never> => ({
      ok: false,
      build: null,
      problem: {
        code: 'network_failed',
        message: `${name} ${what}: ${e instanceof Error ? e.message : String(e)}.`,
        hint: 'Check that `ragondin ui` is still running and reachable, then retry.',
        location: null,
        status,
      },
    });

    let response: Response;
    try {
      response = await fetch(url, init);
    } catch (e) {
      return networkFailed(e, null, 'failed before any answer');
    }
    const build = response.headers.get(BUILD_HEADER);
    let text: string;
    try {
      text = await response.text();
    } catch (e) {
      // The answer began and its body broke off: the connection, not the API.
      return { ...networkFailed(e, response.status, `answered ${response.status}, and its body broke off`), build };
    }
    const type = response.headers.get('content-type') ?? '';
    const failed = (why: string): ApiResult<never> => ({ ok: false, build, problem: unreadable(name, response.status, why) });

    if (type.startsWith('application/problem+json')) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        return failed('its problem body is not JSON');
      }
      if (!isProblem(parsed)) return failed('its problem body lacks a string code, detail or hint');
      return {
        ok: false,
        build,
        problem: { code: parsed.code, message: parsed.detail, hint: parsed.hint, location: parsed.location ?? null, status: response.status },
      };
    }
    if (!response.ok) return failed(`its body is ${type === '' ? 'untyped' : type}, not a problem`);
    if (text === '') {
      // Only an answer the description declares empty may be: a 204, or an
      // operation whose success response has no body.
      const empty = response.status === 204 || EMPTY_ANSWERS.includes(`${verb} ${template}`);
      return empty ? { ok: true, build, value: null as never } : failed('its body is empty where the description declares one');
    }
    try {
      return { ok: true, build, value: JSON.parse(text) as never };
    } catch {
      return failed('its body is not JSON');
    }
  }

  const params = (rest: unknown[]) => rest[0] as Record<string, string> | undefined;
  return {
    get: (path, ...rest) => request('get', path, undefined, params(rest)),
    post: (path, body, ...rest) => request('post', path, body, params(rest)),
    put: (path, body, ...rest) => request('put', path, body, params(rest)),
    patch: (path, body, ...rest) => request('patch', path, body, params(rest)),
    del: (path, ...rest) => request('delete', path, undefined, params(rest)),
  };
}
