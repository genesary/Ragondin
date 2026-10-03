import { API_BASE } from './base.ts';
import { EMPTY_ANSWERS, type Paths, type Problem } from './types.ts';

/**
 * The header every response carries the answering build's identity in —
 * `BUILD_HEADER` in runtime/ragondin-api. The description declares request
 * headers only, never a response's, so this one name is written here rather
 * than generated.
 */
export const BUILD_HEADER = 'x-ragondin-build';

/**
 * The codes the client itself reports, for failures that never reached the
 * API's own error handling: no answer at all, an answer it cannot read, a
 * different build answering, a request it refused to send, and a request its
 * caller cancelled — which a caller drops rather than renders, since it asked
 * for the cancellation.
 */
export type ClientCode = 'network_failed' | 'response_unreadable' | 'build_mismatch' | 'request_invalid' | 'request_aborted';

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
  /** The parameter, path parameter or header a `parameter_invalid` names, when the API knows which; absent otherwise. */
  name?: string;
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
export type Body<P extends keyof Paths, M extends Method> = Operation<P, M> extends { body: infer B } ? B : never;
/** No argument for a path without parameters; the parameters otherwise. */
type ParamsArg<P extends keyof Paths, M extends Method> = Operation<P, M> extends { params: infer Q }
  ? Q extends Record<string, never>
    ? []
    : [params: Q]
  : never;
type QueryOf<O> = O extends { query: infer Q } ? Q : never;
type HeadersOf<O> = O extends { headers: infer H } ? H : never;
/** Whether every member of `T` may be left out. */
type AllOptional<T> = Partial<T> extends T ? true : false;
/** `{ key: T }`, optional when every member of `T` is; nothing when the operation declares no `T`. */
type Field<K extends string, T> = [T] extends [never] ? unknown : AllOptional<T> extends true ? { [Key in K]?: T } : { [Key in K]: T };
/**
 * The query parameters and request headers an operation declares, as the
 * description types them, and the signal that cancels the request — which any
 * operation takes, since it is the caller's and never sent.
 */
export type RequestOptions<P extends keyof Paths, M extends Method> = Field<'query', QueryOf<Operation<P, M>>> & Field<'headers', HeadersOf<Operation<P, M>>> & { signal?: AbortSignal };
/** The options, optional unless a declared query parameter or header is required. */
type OptionsArg<P extends keyof Paths, M extends Method> = AllOptional<RequestOptions<P, M>> extends true ? [options?: RequestOptions<P, M>] : [options: RequestOptions<P, M>];
/** The arguments after the path (and the body): its path parameters, then its query and headers. */
type Args<P extends keyof Paths, M extends Method> = [...ParamsArg<P, M>, ...OptionsArg<P, M>];

export type ApiClient = {
  get<P extends PathWith<'get'>>(path: P, ...args: Args<P, 'get'>): Promise<ApiResult<Answer<P, 'get'>>>;
  post<P extends PathWith<'post'>>(path: P, body: Body<P, 'post'>, ...args: Args<P, 'post'>): Promise<ApiResult<Answer<P, 'post'>>>;
  put<P extends PathWith<'put'>>(path: P, body: Body<P, 'put'>, ...args: Args<P, 'put'>): Promise<ApiResult<Answer<P, 'put'>>>;
  patch<P extends PathWith<'patch'>>(path: P, body: Body<P, 'patch'>, ...args: Args<P, 'patch'>): Promise<ApiResult<Answer<P, 'patch'>>>;
  del<P extends PathWith<'delete'>>(path: P, ...args: Args<P, 'delete'>): Promise<ApiResult<Answer<P, 'delete'>>>;
};

/** What the request builder reads off the arguments after the path. */
type Sent = { params: Record<string, string> | undefined; query: Record<string, unknown> | undefined; headers: Record<string, unknown> | undefined; signal: AbortSignal | undefined };

/**
 * The query string of `query`, serialized by `URLSearchParams` — every
 * reserved character percent-encoded, a space as `+`, which the server reads
 * as one — with an absent parameter left out; empty, without its `?`, when
 * none is left.
 */
function search(query: Record<string, unknown> | undefined): string {
  const encoded = new URLSearchParams();
  for (const [name, value] of Object.entries(query ?? {})) {
    if (value !== undefined && value !== null) encoded.append(name, String(value));
  }
  const text = encoded.toString();
  return text === '' ? '' : `?${text}`;
}

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

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const isEdgeLocation = (value: unknown) => isObject(value) && typeof value.from === 'string' && typeof value.to === 'string' && typeof value.port === 'number';

/**
 * Whether a location is one: absent or null, or an object carrying both of
 * its required members — a node (a string or null) and an edge (null, or its
 * two ends and port). An absent member is no location either.
 */
const isLocation = (value: unknown) => {
  if (value === undefined || value === null) return true;
  if (!isObject(value)) return false;
  return (value.node === null || typeof value.node === 'string') && (value.edge === null || isEdgeLocation(value.edge));
};

/**
 * Whether a parsed problem body has the members every problem carries, and a
 * location a screen can render — never "at node undefined".
 */
const isProblem = (value: unknown): value is Problem =>
  isObject(value) && typeof value.code === 'string' && typeof value.detail === 'string' && typeof value.hint === 'string' && isLocation(value.location);

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
  async function request(method: Method, template: string, body: unknown, { params, query, headers, signal }: Sent): Promise<ApiResult<never>> {
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
    const url = `${API_BASE}${path}${search(query)}`;
    const name = `${verb} ${url}`;
    const sent: Record<string, string> = { accept: 'application/json, application/problem+json' };
    // The headers the description declares for this operation, typed by
    // `Paths`: only the ones given are sent.
    for (const [header, value] of Object.entries(headers ?? {})) {
      if (value !== undefined && value !== null) sent[header] = String(value);
    }
    const init: RequestInit = { method: verb, headers: sent };
    if (signal !== undefined) init.signal = signal;
    if (body !== undefined) {
      init.body = JSON.stringify(body);
      init.headers = { ...sent, 'content-type': 'application/json' };
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

    // Cancelled by its caller, before its answer or while its body was read:
    // told apart by the signal, never by the error's name, which varies.
    const aborted = (status: number | null, build: string | null): ApiResult<never> => ({
      ok: false,
      build,
      problem: { code: 'request_aborted', message: `${name} was cancelled by the view that asked for it.`, hint: 'Nothing to do: a newer request replaced it.', location: null, status },
    });

    let response: Response;
    try {
      response = await fetch(url, init);
    } catch (e) {
      if (signal?.aborted === true) return aborted(null, null);
      return networkFailed(e, null, 'failed before any answer');
    }
    const build = response.headers.get(BUILD_HEADER);
    let text: string;
    try {
      text = await response.text();
    } catch (e) {
      if (signal?.aborted === true) return aborted(response.status, build);
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
      if (!isProblem(parsed)) return failed('its problem body lacks a string code, detail or hint, or has a malformed location');
      const problem: ApiProblem = { code: parsed.code, message: parsed.detail, hint: parsed.hint, location: parsed.location ?? null, status: response.status };
      if (typeof parsed.name === 'string') problem.name = parsed.name;
      return { ok: false, build, problem };
    }
    if (!response.ok) return failed(`its body is ${type === '' ? 'untyped' : type}, not a problem`);
    // Only an operation the description declares empty may answer empty — a
    // 204 included — or its `null` would stand in for a type it is not.
    if (text === '' || response.status === 204) {
      return EMPTY_ANSWERS.includes(`${verb} ${template}`) ? { ok: true, build, value: null as never } : failed('its body is empty where the description declares one');
    }
    try {
      return { ok: true, build, value: JSON.parse(text) as never };
    } catch {
      return failed('its body is not JSON');
    }
  }

  // A path with a `{name}` takes its parameters first; the options — the
  // query and the headers, when the operation declares either, and the
  // signal — come after.
  const sent = (template: string, rest: unknown[]): Sent => {
    const hasParams = template.includes('{');
    const options = (rest[hasParams ? 1 : 0] ?? {}) as { query?: Record<string, unknown>; headers?: Record<string, unknown>; signal?: AbortSignal };
    return { params: hasParams ? (rest[0] as Record<string, string>) : undefined, query: options.query, headers: options.headers, signal: options.signal };
  };
  return {
    get: (path, ...rest) => request('get', path, undefined, sent(path, rest)),
    post: (path, body, ...rest) => request('post', path, body, sent(path, rest)),
    put: (path, body, ...rest) => request('put', path, body, sent(path, rest)),
    patch: (path, body, ...rest) => request('patch', path, body, sent(path, rest)),
    del: (path, ...rest) => request('delete', path, undefined, sent(path, rest)),
  };
}
