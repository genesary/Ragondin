// The URL state contract: the six screens and the state each carries in the
// hash, so a link pasted into an issue reproduces the view (the front-end
// design, § 3). A screen reads its state from here and never invents a shape;
// one that needs more extends its route here, in its own issue.
// ARCHITECTURE.md § The router and the URL state.
import { useSyncExternalStore } from 'react';

export type Route =
  /** `#runs`: the workspace's runs. An empty hash shows it. */
  | { screen: 'runs' }
  /** `#pipeline/<name>`: one pipeline's node × benchmark matrix; `#pipeline` before one is chosen. */
  | { screen: 'pipeline'; name?: string }
  /**
   * `#compare/<id>+<id>…?baseline=<id>`: runs side by side, against a
   * baseline; `#compare`, with no ids, before any is chosen.
   */
  | { screen: 'compare'; ids: string[]; baseline?: string }
  /** `#replay`: before a query is chosen. */
  | { screen: 'replay' }
  /** `#replay/<run>/q/<query>?with=<run>`: one query of one run, node by node, optionally beside another run. */
  | { screen: 'replay'; run: string; query: string; with?: string }
  /** `#editor/<name>`: one pipeline, edited on the canvas; `#editor` before one is opened. */
  | { screen: 'editor'; name?: string }
  /** `#setup`: benchmarks and services. */
  | { screen: 'setup' };

export type ScreenName = Route['screen'];

const enc = encodeURIComponent;

/** The hash that shows `route`, every value encoded. */
export function formatHash(route: Route): string {
  switch (route.screen) {
    case 'runs':
    case 'setup':
      return `#${route.screen}`;
    case 'pipeline':
    case 'editor':
      return route.name === undefined ? `#${route.screen}` : `#${route.screen}/${enc(route.name)}`;
    case 'compare': {
      if (route.ids.length === 0) return '#compare';
      const query = route.baseline === undefined ? '' : `?baseline=${enc(route.baseline)}`;
      return `#compare/${route.ids.map(enc).join('+')}${query}`;
    }
    case 'replay': {
      if (!('run' in route)) return '#replay';
      const query = route.with === undefined ? '' : `?with=${enc(route.with)}`;
      return `#replay/${enc(route.run)}/q/${enc(route.query)}${query}`;
    }
  }
}

/**
 * Whether a decoded value can name something: not empty, and not `.` or `..`,
 * which a screen would pass into an API path where they name another path.
 */
const isValue = (value: string) => value !== '' && value !== '.' && value !== '..';

/**
 * The route a hash shows, or null for one that names no screen. Never a
 * guess: a malformed address is no route, and the shell says so.
 */
export function parseHash(hash: string): Route | null {
  const raw = hash.startsWith('#') ? hash.slice(1) : hash;
  const [path = '', search = ''] = raw.split(/\?(.*)/s);
  const query = new URLSearchParams(search);
  let segments: string[];
  try {
    segments = path === '' ? [] : path.split('/').map((s) => decodeURIComponent(s));
  } catch {
    // A malformed escape (`%E0%A4%A`) names nothing: no route, not an exception.
    return null;
  }
  const [screen, ...rest] = segments;
  const nonEmpty = rest.every(isValue);

  switch (screen) {
    case undefined:
      return { screen: 'runs' };
    case 'runs':
    case 'setup':
      return rest.length === 0 ? { screen } : null;
    case 'pipeline':
    case 'editor':
      if (rest.length === 0) return { screen };
      return rest.length === 1 && nonEmpty ? { screen, name: rest[0] as string } : null;
    case 'compare': {
      const baseline = query.get('baseline');
      if (rest.length === 0) return baseline === null ? { screen, ids: [] } : null;
      if (baseline !== null && !isValue(baseline)) return null;
      // Split before decoding, so an encoded `+` stays inside its id.
      const ids = (path.split('/')[1] ?? '').split('+').map((id) => decodeURIComponent(id));
      if (rest.length !== 1 || !ids.every(isValue)) return null;
      return baseline === null ? { screen, ids } : { screen, ids, baseline };
    }
    case 'replay': {
      const other = query.get('with');
      if (rest.length === 0) return other === null ? { screen } : null;
      if (rest.length !== 3 || rest[1] !== 'q' || !nonEmpty || (other !== null && !isValue(other))) return null;
      const [run, , q] = rest as [string, string, string];
      return other === null ? { screen, run, query: q } : { screen, run, query: q, with: other };
    }
    default:
      return null;
  }
}

const subscribe = (onChange: () => void) => {
  window.addEventListener('hashchange', onChange);
  return () => window.removeEventListener('hashchange', onChange);
};
const currentHash = () => window.location.hash;

/** The route the address shows now, following every change to the hash. */
export function useRoute(): Route | null {
  return parseHash(useSyncExternalStore(subscribe, currentHash));
}

/**
 * Shows `route`: as a new entry in the browser's history, or with `replace`
 * in place of the current one, so Back skips it — for a correction such as a
 * default filled in, not for a move the user made.
 */
export function navigate(route: Route, { replace = false }: { replace?: boolean } = {}) {
  const hash = formatHash(route);
  if (!replace) {
    window.location.hash = hash;
    return;
  }
  const oldURL = window.location.href;
  window.history.replaceState(window.history.state, '', hash);
  // Replacing the entry announces nothing by itself; the router follows the
  // hash through this event, as it does after a user's edit.
  window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL, newURL: window.location.href }));
}
