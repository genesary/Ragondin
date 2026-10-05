// The URL state contract: the six screens and the state each carries in the
// hash, so a link pasted into an issue reproduces the view (the front-end
// design, § 3). A screen reads its state from here and never invents a shape;
// one that needs more extends its route here, in its own issue.
// ARCHITECTURE.md § The router and the URL state.
import { useSyncExternalStore } from 'react';

export type Route =
  /**
   * `#runs?sel=<id>,<id>…`: the workspace's runs, and the runs selected for
   * Compare in the order they were checked; `#runs`, or an empty hash, with
   * none selected. `#runs/job/<id>` shows one job of the queue among them
   * — the address a toast's "open" and a pasted link land on — with the
   * same selection beside it. `#runs?launch=<pipeline>&up_to=<node>`
   * opens the launch panel on that pipeline, cut at that node — where the
   * editor's "Run up to this node" lands — or, without `up_to`, on the whole
   * pipeline; `&benchmark=<name>` opens it on that benchmark too — where the
   * Pipeline screen's Run lands — and the key repeated, one value each, on
   * several, in order — where "Run the N missing cells" lands. Beside a
   * selection or a job.
   */
  | { screen: 'runs'; sel?: string[]; job?: string; launch?: { pipeline: string; upTo?: string; benchmarks?: string[] } }
  /** `#pipeline/<name>`: one pipeline's node × benchmark matrix; `#pipeline` before one is chosen. */
  | { screen: 'pipeline'; name?: string }
  /**
   * `#compare/<id>+<id>…?baseline=<id>`: runs side by side, against a
   * baseline; `#compare`, with no ids, before any is chosen.
   */
  | { screen: 'compare'; ids: string[]; baseline?: string }
  /** `#replay`: before a run is chosen. */
  | { screen: 'replay' }
  /** `#replay/<run>`: one run, before a query is chosen. */
  | { screen: 'replay'; run: string; query?: never }
  /**
   * `#replay/<run>/q/<query>/node/<id>?with=<run>`: one query of one run,
   * node by node, the node selected on it, optionally beside another run;
   * without `/node/<id>`, none selected.
   */
  | { screen: 'replay'; run: string; query: string; node?: string; with?: string }
  /**
   * `#replay/job/<id>/q/<query>/node/<id>`: one query of a failed or
   * cancelled run job's partial traces, alone — never a stored run — and the
   * node selected on it; `#replay/job/<id>` before a query is chosen.
   */
  | { screen: 'replay'; job: string; query?: string; node?: string }
  /** `#editor`: before a pipeline is opened. */
  | { screen: 'editor'; name?: never; node?: never }
  /** `#editor/<name>/node/<id>`: one pipeline, edited on the canvas, and the node selected on it; `#editor/<name>` with none. */
  | { screen: 'editor'; name: string; node?: string }
  /**
   * `#setup/<section>`: the workspace, its benchmarks and services, and this
   * build, scrolled to and focusing one section; `#setup` at the top.
   */
  | { screen: 'setup'; section?: SetupSection };

/** The sections of Setup an address can focus. */
export const SETUP_SECTIONS = ['benchmarks', 'services'] as const;
export type SetupSection = (typeof SETUP_SECTIONS)[number];

export type ScreenName = Route['screen'];

const enc = encodeURIComponent;

/** The hash that shows `route`, every value encoded. */
export function formatHash(route: Route): string {
  switch (route.screen) {
    case 'runs': {
      const path = route.job === undefined ? '#runs' : `#runs/job/${enc(route.job)}`;
      const query = [
        ...(route.sel === undefined || route.sel.length === 0 ? [] : [`sel=${route.sel.map(enc).join(',')}`]),
        ...(route.launch === undefined ? [] : [`launch=${enc(route.launch.pipeline)}`]),
        ...(route.launch?.upTo === undefined ? [] : [`up_to=${enc(route.launch.upTo)}`]),
        ...(route.launch?.benchmarks ?? []).map((b) => `benchmark=${enc(b)}`),
      ];
      return query.length === 0 ? path : `${path}?${query.join('&')}`;
    }
    case 'setup':
      return route.section === undefined ? '#setup' : `#setup/${route.section}`;
    case 'pipeline':
      return route.name === undefined ? '#pipeline' : `#pipeline/${enc(route.name)}`;
    case 'editor':
      if (route.name === undefined) return '#editor';
      return route.node === undefined ? `#editor/${enc(route.name)}` : `#editor/${enc(route.name)}/node/${enc(route.node)}`;
    case 'compare': {
      if (route.ids.length === 0) return '#compare';
      const query = route.baseline === undefined ? '' : `?baseline=${enc(route.baseline)}`;
      return `#compare/${route.ids.map(enc).join('+')}${query}`;
    }
    case 'replay': {
      const node = 'node' in route && route.node !== undefined ? `/node/${enc(route.node)}` : '';
      if ('job' in route) return route.query === undefined ? `#replay/job/${enc(route.job)}` : `#replay/job/${enc(route.job)}/q/${enc(route.query)}${node}`;
      if (!('run' in route)) return '#replay';
      if (route.query === undefined) return `#replay/${enc(route.run)}`;
      const query = route.with === undefined ? '' : `?with=${enc(route.with)}`;
      return `#replay/${enc(route.run)}/q/${enc(route.query)}${node}${query}`;
    }
  }
}

/**
 * The view a route shows, apart from the state within it: its hash up to the
 * query. A change of query alone — Runs' selection, Compare's baseline — is
 * state within one view, and the shell moves no focus for it. Replay's view
 * is its run: the query shown and the run beside it are state within it, so
 * the arrow keys move through the queries without focus leaving the list.
 */
export function viewOf(route: Route): string {
  if (route.screen === 'replay' && 'run' in route) return formatHash({ screen: 'replay', run: route.run });
  if (route.screen === 'replay' && 'job' in route) return formatHash({ screen: 'replay', job: route.job });
  // Setup's section is a place on one page, not another view: moving to it
  // focuses the section, and the shell must not take focus to the heading.
  if (route.screen === 'setup') return '#setup';
  // The node selected is state within the pipeline's view, as Replay's query is.
  if (route.screen === 'editor' && route.name !== undefined) return formatHash({ screen: 'editor', name: route.name });
  // The job shown is state within Runs, as a selection is: the screen moves focus to it itself.
  if (route.screen === 'runs') return '#runs';
  return formatHash(route).split('?')[0] as string;
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
    case 'runs': {
      const job = rest.length === 2 && rest[0] === 'job' && nonEmpty ? (rest[1] as string) : undefined;
      if (rest.length !== 0 && job === undefined) return null;
      // The launch panel's pipeline, the node it is cut at and the benchmark it opens on: a node or a benchmark with no
      // pipeline names nothing.
      const pipeline = query.get('launch');
      const upTo = query.get('up_to');
      const benchmarks = query.getAll('benchmark');
      if (pipeline === null ? upTo !== null || benchmarks.length > 0 : ![pipeline, upTo, ...benchmarks].every((v) => v === null || isValue(v))) return null;
      const launch = pipeline === null ? {} : { launch: { pipeline, ...(upTo === null ? {} : { upTo }), ...(benchmarks.length === 0 ? {} : { benchmarks }) } };
      const at = { ...(job === undefined ? {} : { job }), ...launch };
      // Read from the raw query, split before decoding, so an encoded `,`
      // stays inside its id.
      const raw = search.split('&').find((pair) => pair.startsWith('sel='));
      if (raw === undefined) return { screen, ...at };
      let sel: string[];
      try {
        sel = raw.slice('sel='.length).split(',').map((id) => decodeURIComponent(id));
      } catch {
        return null;
      }
      return sel.every(isValue) ? { screen, ...at, sel } : null;
    }
    case 'setup': {
      if (rest.length === 0) return { screen };
      const section = SETUP_SECTIONS.find((s) => s === rest[0]);
      return rest.length === 1 && section !== undefined ? { screen, section } : null;
    }
    case 'pipeline':
      if (rest.length === 0) return { screen };
      return rest.length === 1 && nonEmpty ? { screen, name: rest[0] as string } : null;
    case 'editor':
      if (rest.length === 0) return { screen };
      if (!nonEmpty) return null;
      if (rest.length === 1) return { screen, name: rest[0] as string };
      return rest.length === 3 && rest[1] === 'node' ? { screen, name: rest[0] as string, node: rest[2] as string } : null;
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
      // A run is named by its hash, so `job` is never one: it opens a job's partial traces.
      if (rest[0] === 'job') {
        if (other !== null || !nonEmpty) return null;
        if (rest.length === 2) return { screen, job: rest[1] as string };
        if (rest[2] !== 'q') return null;
        if (rest.length === 4) return { screen, job: rest[1] as string, query: rest[3] as string };
        return rest.length === 6 && rest[4] === 'node' ? { screen, job: rest[1] as string, query: rest[3] as string, node: rest[5] as string } : null;
      }
      if (rest.length === 1) return other === null && nonEmpty ? { screen, run: rest[0] as string } : null;
      // The node selected comes after the query: `/q/<query>/node/<id>`.
      const named = rest.length === 5 && rest[3] === 'node';
      if ((rest.length !== 3 && !named) || rest[1] !== 'q' || !nonEmpty || (other !== null && !isValue(other))) return null;
      const [run, , q, , node] = rest as [string, string, string, string?, string?];
      return { screen, run, query: q, ...(named ? { node: node as string } : {}), ...(other === null ? {} : { with: other }) };
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
