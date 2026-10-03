// Test doubles for the network, for tests anywhere under ui/: a fake event
// stream, and request-level mocks of the API typed by the generated
// description. They live here because only src/api/ may name a network
// primitive (ARCHITECTURE.md § The network lint). No application module
// imports this file, so nothing of it reaches the bundle.
import { vi } from 'vitest';
import { API_BASE } from './base.ts';
import { BUILD_HEADER, type Answer, type Body, type PathWith } from './client.ts';
import type { Problem } from './types.ts';

/** An `EventSource` a test drives by hand. */
export class FakeEventSource {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  static instances: FakeEventSource[] = [];

  static latest(): FakeEventSource {
    const last = FakeEventSource.instances.at(-1);
    if (last === undefined) throw new Error('no EventSource was opened');
    return last;
  }

  readonly url: string;
  readyState = FakeEventSource.CONNECTING;
  closed = false;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  /** The connection opens (or reopens, after `fail`). */
  open() {
    this.readyState = FakeEventSource.OPEN;
    this.onopen?.(new Event('open'));
  }

  emit(data: string) {
    this.onmessage?.(new MessageEvent('message', { data }));
  }

  /** The connection drops and the browser will retry it by itself. */
  fail() {
    this.readyState = FakeEventSource.CONNECTING;
    this.onerror?.(new Event('error'));
  }

  /** The connection drops and the browser gives up on it. */
  drop() {
    this.readyState = FakeEventSource.CLOSED;
    this.onerror?.(new Event('error'));
  }

  close() {
    this.readyState = FakeEventSource.CLOSED;
    this.closed = true;
  }
}

/** Replaces the global `EventSource`; `vi.unstubAllGlobals()` restores it. */
export function installFakeEventSource() {
  FakeEventSource.instances = [];
  vi.stubGlobal('EventSource', FakeEventSource);
}

/** What a mocked path answers: a body of its generated type, a problem, or no answer at all. */
export type MockReply<T> = { body: T; build?: string } | { problem: Problem; build?: string } | { network: string };

/**
 * The mocked answers, keyed by method and the description's own path
 * template: a key the description does not have, or a body that is not its
 * path's generated type, does not compile. A list is answered in order, its
 * last reply repeating. A GET may answer from the query string and the path
 * it was sent — one template, two ids — and a POST from the body it was sent, typed as its path's request body —
 * and later, through a promise the test resolves, so two answers can be made
 * to arrive out of order.
 */
export type MockRoutes = {
  [P in PathWith<'get'> as `GET ${P}`]?: MockReply<Answer<P, 'get'>> | MockReply<Answer<P, 'get'>>[] | ((query: URLSearchParams, path: string) => MockReply<Answer<P, 'get'>> | Promise<MockReply<Answer<P, 'get'>>>);
} & {
  [P in PathWith<'post'> as `POST ${P}`]?: MockReply<Answer<P, 'post'>> | MockReply<Answer<P, 'post'>>[] | ((body: Body<P, 'post'>) => MockReply<Answer<P, 'post'>> | Promise<MockReply<Answer<P, 'post'>>>);
};

// A path template as a pattern over the whole path, which a query string may
// follow: the client writes one after `?` (ARCHITECTURE.md § The client).
const pattern = (template: string) => new RegExp(`^${API_BASE}${template.replace(/[.*+?^$()|[\]\\]/g, '\\$&').replace(/\{[^}]+\}/g, '[^/?]+')}(?:\\?.*)?$`);

/**
 * Replaces the global `fetch` with answers from `routes`, each carrying the
 * build identity `build` unless the reply names its own. A request no route
 * matches is answered as the server answers one: `route_not_found`. Returns
 * the requests made, as `GET /api/v1/…`, and beside each the JSON body it
 * sent, parsed — undefined for none. `vi.unstubAllGlobals()` restores
 * `fetch`.
 */
export function mockApi(routes: MockRoutes, { build = 'test-build' }: { build?: string } = {}) {
  const requests: string[] = [];
  const bodies: unknown[] = [];
  // A function is handed the request's query string and path for a GET, its parsed body otherwise.
  type Reply = (sent: unknown, path?: string) => MockReply<unknown> | Promise<MockReply<unknown>>;
  type Replies = MockReply<unknown>[] | Reply;
  const table = Object.entries(routes).map(([key, replies]) => {
    const [method = '', template = ''] = key.split(' ');
    const list: Replies = typeof replies === 'function' ? (replies as Reply) : ((Array.isArray(replies) ? [...replies] : [replies]) as MockReply<unknown>[]);
    return { method, match: pattern(template), replies: list };
  });
  const answer = (status: number, type: string, body: unknown, identity: string) =>
    new Response(JSON.stringify(body), { status, headers: { 'content-type': type, [BUILD_HEADER]: identity } });

  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    const sent: unknown = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
    requests.push(`${method} ${url}`);
    bodies.push(sent);
    const route = table.find((r) => r.method === method && r.match.test(url));
    const replies = route?.replies;
    const [path = url, search = ''] = url.split(/\?(.*)/s);
    const reply = replies === undefined ? undefined : typeof replies === 'function' ? await (method === 'GET' ? replies(new URLSearchParams(search), path) : replies(sent)) : replies.length > 1 ? replies.shift() : replies[0];
    if (reply === undefined) {
      const problem: Problem = {
        type: 'urn:ragondin:problem:route_not_found',
        title: 'Route not found',
        status: 404,
        detail: `No route answers ${method} ${url} in this mock.`,
        code: 'route_not_found',
        hint: 'Mock the path in the test.',
      };
      return answer(404, 'application/problem+json', problem, build);
    }
    if ('network' in reply) throw new TypeError(reply.network);
    if ('problem' in reply) return answer(reply.problem.status, 'application/problem+json', reply.problem, reply.build ?? build);
    return answer(200, 'application/json', reply.body, reply.build ?? build);
  });
  return { requests, bodies };
}
