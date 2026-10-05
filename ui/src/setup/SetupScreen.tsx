// The Setup screen: everything a run needs besides its pipeline (the
// front-end design, § 3 and § 6) — the workspace, the benchmarks, the
// services, and what this build can run — or, on a workspace with no
// benchmark and no service, the first-launch invitation. ARCHITECTURE.md
// § The Setup screen.
import { useCallback, useEffect, useId, useMemo, useRef, useState, type RefObject } from 'react';
import { Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { Jobs } from '../api/jobs.ts';
import type { BenchmarkEntry, Scorable, ServiceBinding, ServiceStatus, Workspace } from '../api/types.ts';
import { useJobEvents, useJobQueue, useJobs } from '../jobs/queue.tsx';
import type { SetupSection } from '../routes.ts';
import { Loading, type RequestState } from '../shell/states.tsx';
import { Benchmarks, type Downloads } from './Benchmarks.tsx';
import { FirstLaunch, NextStep } from './FirstLaunch.tsx';
import { EMPTY_DRAFT, type ConnectDraft } from './forms.tsx';
import { downloadView, finishedDownloads, isFirstLaunch, serviceKey, type Submission } from './model.ts';
import { displayed, Services, type RemovedSlot, type SessionProbe } from './Services.tsx';
import './Setup.css';
import { BuildSection, WorkspaceSection } from './Workspace.tsx';

export type SetupScreenProps = {
  client: ApiClient;
  /** The workspace as the shell read it — the read the build identity handshake judged. */
  workspace: RequestState<Workspace>;
  /** Reads the workspace again, keeping it on screen: after a write, so its counts and the top bar stay current. */
  refreshWorkspace: () => void;
  /** Reads the workspace again after a failure, from its loading state. */
  retryWorkspace: () => void;
  /** The section the address focuses. */
  section?: SetupSection | undefined;
  /** The shell's level-one heading, which it focuses on a move to this screen: drawn here, as the screen's visible name. */
  heading?: RefObject<HTMLHeadingElement | null>;
};

type Mode = 'first' | 'sections';

/**
 * How long a removed binding can be brought back by Undo before the removal
 * is written. Nothing is written meanwhile, so Undo restores the binding, its
 * place in `workspace.toml` and its comments exactly.
 */
export const UNDO_WINDOW_MS = 10_000;

/**
 * What one read of a listing came to: whether its answer landed and was a
 * success; `replaced` when a write's answer overtook it and no read came after.
 */
type ReadOutcome = boolean | 'replaced';

/**
 * Reads a listing, keeping only the answer to the last read asked for; lets
 * a write replace it in place, and a change elsewhere read it again in place.
 */
function useListing<T>(read: () => Promise<{ ok: true; value: T } | { ok: false; problem: ApiProblem }>) {
  const [state, setState] = useState<RequestState<T>>({ status: 'loading' });
  // A read in place that failed: the listing it would have replaced stays on screen, and this says why it is not newer.
  const [stale, setStale] = useState<ApiProblem | null>(null);
  // Every read and every write's answer takes the next number; only the latest lands.
  const latest = useRef(0);
  // The last read issued, by number, and what it came to.
  const lastRead = useRef<{ n: number; outcome: Promise<ReadOutcome> } | null>(null);
  /**
   * Issues one read. Overtaken by a later read, it comes to that read's
   * outcome, since the later one decides what the listing holds — never to a
   * success it did not have; overtaken by a write's answer with no read after
   * it, to `replaced`. In place, a failure is said beside the listing
   * (`stale`) and never replaces one that loaded.
   */
  const issue = useCallback(
    (inPlace: boolean): Promise<ReadOutcome> => {
      const n = ++latest.current;
      const outcome = (async (): Promise<ReadOutcome> => {
        const result = await read();
        if (n !== latest.current) {
          const later = lastRead.current;
          return later !== null && later.n > n ? later.outcome : 'replaced';
        }
        if (result.ok) {
          setStale(null);
          setState({ status: 'loaded', value: result.value });
        } else if (inPlace) {
          setStale(result.problem);
          setState((prev) => (prev.status === 'loaded' ? prev : { status: 'error', problem: result.problem }));
        } else {
          setStale(null);
          setState({ status: 'error', problem: result.problem });
        }
        return result.ok;
      })();
      lastRead.current = { n, outcome };
      return outcome;
    },
    [read],
  );
  const load = useCallback(() => void issue(false), [issue]);
  /**
   * Reads again, keeping the listing on screen. A write's answer that
   * overtakes the read sends it again, since it began before the write and
   * its purpose — a change made elsewhere — is not in the write's answer.
   * Resolves whether the listing now holds a successful answer read after the
   * call.
   */
  const reload = useCallback(async (): Promise<boolean> => {
    for (;;) {
      const outcome = await issue(true);
      if (outcome !== 'replaced') return outcome;
    }
  }, [issue]);
  useEffect(() => {
    void load();
  }, [load]);
  const retry = () => {
    setState({ status: 'loading' });
    load();
  };
  /** A write's answer replaces the listing, and overtakes any read still in flight. */
  const replace = useCallback((value: T | ((prev: T) => T)) => {
    latest.current += 1;
    setState((prev) => {
      if (typeof value !== 'function') return { status: 'loaded', value };
      return prev.status === 'loaded' ? { status: 'loaded', value: (value as (prev: T) => T)(prev.value) } : prev;
    });
  }, []);
  return { state, stale, retry, replace, reload };
}

export function SetupScreen({ client, workspace, refreshWorkspace, retryWorkspace, section, heading }: SetupScreenProps) {
  // The listing's `scorable` — the rule, served — set before the benchmarks it qualifies land.
  const [scorable, setScorable] = useState<Scorable | null>(null);
  const readBenchmarks = useCallback(async () => {
    const r = await client.get('/benchmarks');
    if (r.ok) setScorable(r.value.scorable);
    return r.ok ? { ok: true as const, value: r.value.benchmarks as readonly BenchmarkEntry[] } : r;
  }, [client]);
  const readServices = useCallback(async () => {
    const r = await client.get('/services');
    return r.ok ? { ok: true as const, value: r.value.services as readonly ServiceStatus[] } : r;
  }, [client]);
  const benchmarks = useListing(readBenchmarks);
  const services = useListing(readServices);

  const [probes, setProbes] = useState<ReadonlyMap<string, SessionProbe>>(new Map());
  const [testing, setTesting] = useState<ReadonlySet<string>>(new Set());
  const [models, setModels] = useState<ReadonlyMap<string, string>>(new Map());
  const [connecting, setConnecting] = useState<ServiceStatus | null>(null);
  const [slots, setSlots] = useState<readonly RemovedSlot[]>([]);
  const [refusal, setRefusal] = useState<ApiProblem | null>(null);
  const undoId = useId();
  const rows = useRef(new Map<string, HTMLLIElement>());
  const benchmarksAnchor = useRef<HTMLElement>(null);
  const servicesAnchor = useRef<HTMLElement>(null);

  // First launch or the four sections, decided once both listings are read;
  // kept while one is read again, so a Retry never collapses the page.
  // Derived during render from the previous decision (React's pattern for
  // state that follows other state), never written to a ref.
  const [mode, setMode] = useState<Mode | null>(null);
  const loaded = workspace.status === 'loaded' ? workspace.value : null;
  // The workspace read current when this page saw the first launch end, or
  // undefined while it has not: the sections then say what is in place and
  // the next step, so the way on does not go with the invitation — once the
  // workspace is read again after the write that ended it, so its counts
  // include what that write added.
  const [endedAt, setEndedAt] = useState<Workspace | null | undefined>(undefined);
  const next: Mode | null =
    benchmarks.state.status === 'loaded' && services.state.status === 'loaded'
      ? isFirstLaunch(benchmarks.state.value, services.state.value)
        ? 'first'
        : 'sections'
      : mode === null && (benchmarks.state.status === 'error' || services.state.status === 'error')
        ? 'sections'
        : mode;
  if (next !== mode) {
    if (mode === 'first' && next === 'sections') setEndedAt(loaded);
    setMode(next);
  }
  // The Connect form's fields, kept here so the first launch's form and the
  // Services section's are one draft: the first launch ending loses nothing typed.
  const [draft, setDraft] = useState<ConnectDraft>(EMPTY_DRAFT);

  // The address's section is scrolled to and focused once it is on the page:
  // on load, and on every move to another section.
  const focusedFor = useRef<SetupSection | null>(null);
  useEffect(() => {
    if (section === undefined) {
      focusedFor.current = null;
      return;
    }
    const target = section === 'benchmarks' ? benchmarksAnchor.current : servicesAnchor.current;
    if (target === null || focusedFor.current === section) return;
    focusedFor.current = section;
    target.scrollIntoView?.({ block: 'start' });
    target.focus({ preventScroll: true });
  }, [section, mode]);

  // Where focus goes once the render that puts it on the page has committed:
  // a row after its probe or its Undo, Undo after the row it replaces is gone,
  // or the Benchmarks section after an import ends the first launch and takes
  // the form that had focus with it.
  // `ifLost` is a control that went with a render: a benchmark whose Download
  // went with its row, or `null` for one with no slot (the inline Retry).
  // Focus moves only if it is still on the page's body, or on that row's
  // empty slot, so a person who moved on meanwhile keeps their place.
  const [pendingFocus, setPendingFocus] = useState<{ row: string } | { undo: true } | { benchmarks: true; ifLost?: string | null } | null>(null);
  useEffect(() => {
    if (pendingFocus === null) return;
    if ('ifLost' in pendingFocus && pendingFocus.ifLost !== undefined) {
      const active = document.activeElement;
      const slot = pendingFocus.ifLost;
      const lost = active === null || active === document.body || (slot !== null && active.closest('.rg-setup__action')?.getAttribute('data-benchmark') === slot);
      if (!lost) {
        setPendingFocus(null);
        return;
      }
    }
    const target = 'row' in pendingFocus ? rows.current.get(pendingFocus.row) : 'undo' in pendingFocus ? document.getElementById(undoId) : benchmarksAnchor.current;
    if (target === null || target === undefined) return;
    target.focus();
    setPendingFocus(null);
  });
  const focusRow = (key: string) => setPendingFocus({ row: key });

  const probe = async (family: string, name: string, uri: string, servedModel: string) => {
    const key = serviceKey({ family, name });
    setTesting((t) => new Set(t).add(key));
    const result = await client.post('/services/{family}/{name}/probe', servedModel === '' ? {} : { served_model: servedModel }, { family, name });
    const outcome: SessionProbe['outcome'] = result.ok ? { ok: true, identity: result.value.identity } : { ok: false, problem: result.problem };
    setProbes((p) => new Map(p).set(key, { uri, at: new Date(), outcome }));
    // The server learnt the same; its listing is not read again for it.
    services.replace((list) => list.map((s) => (serviceKey(s) === key && s.uri === uri ? { ...s, connected: result.ok, identity: result.ok ? result.value.identity : s.identity } : s)));
    setTesting((t) => {
      const next = new Set(t);
      next.delete(key);
      return next;
    });
    refreshWorkspace();
  };

  // The removal whose Undo window is open: nothing is written until it closes.
  // Its timer is null while a write binding the same name again is in flight:
  // the window is paused then, and that write's answer cancels it (stored) or
  // reopens it (refused) — unless something else closed it first, which
  // writes it through the read before the DELETE.
  const open = useRef<{ binding: ServiceBinding; timer: ReturnType<typeof setTimeout> | null } | null>(null);
  const slotEls = useRef(new Map<string, HTMLLIElement>());
  /** Whether focus is inside the slot standing for `key`, Undo included. */
  const focusIn = (key: string) => slotEls.current.get(key)?.contains(document.activeElement) ?? false;

  // The shell's refresh is a new function on each of its renders; a write that
  // lands after this screen re-rendered, or left, calls the latest.
  const refresh = useRef(refreshWorkspace);
  useEffect(() => {
    refresh.current = refreshWorkspace;
  });
  const replaceServices = services.replace;

  const write = useCallback(
    async (binding: ServiceBinding) => {
      const key = serviceKey(binding);
      const mark = (state: RemovedSlot['state'], now?: string) => setSlots((all) => all.map((s) => (serviceKey(s.binding) === key ? { ...s, state, ...(now === undefined ? {} : { now }) } : s)));
      // Undo is about to go: focus on it moves to the slot itself, not to the page.
      if (focusIn(key)) slotEls.current.get(key)?.focus();
      mark('writing');
      /** The binding is still there: its row comes back, focus with it if it was in the slot, and the refusal says why. */
      const restore = (problem: ApiProblem) => {
        if (focusIn(key)) setPendingFocus({ row: key });
        setSlots((all) => all.filter((s) => serviceKey(s.binding) !== key));
        setRefusal(problem);
      };
      // A DELETE removes by name, and the window lasted seconds: another tab,
      // the CLI or a hand edit may have bound the name again meanwhile. What is
      // bound now is read first, and only the binding that was removed is deleted.
      const now = await client.get('/services');
      if (!now.ok) {
        restore(now.problem);
        return;
      }
      const current = now.value.services.find((s) => serviceKey(s) === key);
      if (current === undefined || current.uri !== binding.uri) {
        replaceServices(now.value.services);
        mark(current === undefined ? 'removed' : 'kept', current?.uri);
        refresh.current();
        return;
      }
      const result = await client.del('/services/{family}/{name}', { family: binding.family, name: binding.name });
      if (!result.ok) {
        restore(result.problem);
        return;
      }
      replaceServices(result.value.services);
      mark('removed');
      refresh.current();
    },
    [client, replaceServices],
  );

  /**
   * Closes the open Undo window, writing its removal now — a paused one too,
   * whenever something else closes it (another removal, leaving the screen or
   * the page). The read before the DELETE makes that safe: if the re-bind
   * that paused it has stored the name at its new address, nothing is deleted.
   */
  const closeWindow = useCallback(() => {
    const pending = open.current;
    if (pending === null) return;
    if (pending.timer !== null) clearTimeout(pending.timer);
    open.current = null;
    void write(pending.binding);
  }, [write]);

  // Leaving the screen, or the page, writes a pending removal rather than
  // dropping it: the person asked for it, and Undo is no longer on screen.
  useEffect(() => {
    window.addEventListener('pagehide', closeWindow);
    return () => {
      window.removeEventListener('pagehide', closeWindow);
      closeWindow();
    };
  }, [closeWindow]);

  const onRemove = (service: ServiceStatus) => {
    // One window at a time: removing another binding writes the pending one now.
    closeWindow();
    const binding: ServiceBinding = { family: service.family, name: service.name, uri: service.uri };
    const key = serviceKey(service);
    // Measured before the row goes, so its slot holds its place.
    const height = rows.current.get(key)?.getBoundingClientRect().height ?? 0;
    const list = services.state.status === 'loaded' ? services.state.value : [];
    const index = Math.max(
      displayed(list, slots).findIndex((row) => !('binding' in row) && serviceKey(row) === key),
      0,
    );
    setSlots((all) => [...all.filter((s) => serviceKey(s.binding) !== key), { binding, index, height, state: 'pending' }]);
    setRefusal(null);
    open.current = { binding, timer: setTimeout(closeWindow, UNDO_WINDOW_MS) };
    setPendingFocus({ undo: true });
  };

  const onUndo = (slot: RemovedSlot) => {
    const key = serviceKey(slot.binding);
    if (open.current === null || serviceKey(open.current.binding) !== key) return;
    if (open.current.timer !== null) clearTimeout(open.current.timer);
    open.current = null;
    setSlots((all) => all.filter((s) => serviceKey(s.binding) !== key));
    focusRow(key);
  };

  const onConnect = async ({ family, name, uri, servedModel }: { family: string; name: string; uri: string; servedModel: string }) => {
    const key = serviceKey({ family, name });
    // Binding a name whose removal is pending pauses that removal: a stored
    // binding cancels it, a refused one opens the window again.
    const paused = open.current !== null && serviceKey(open.current.binding) === key ? open.current : null;
    if (paused !== null && paused.timer !== null) {
      clearTimeout(paused.timer);
      paused.timer = null;
    }
    // Drawn as its row from the click, so the answer moves nothing.
    setConnecting({ family, name, uri, connected: false, identity: null });
    setModels((m) => new Map(m).set(key, servedModel));
    const put = await client.put('/services/{family}/{name}', { uri }, { family, name });
    if (!put.ok) {
      setConnecting(null);
      if (paused !== null && open.current === paused) paused.timer = setTimeout(closeWindow, UNDO_WINDOW_MS);
      return put.problem;
    }
    if (paused !== null && open.current === paused) open.current = null;
    services.replace(put.value.services);
    setSlots((all) => all.filter((s) => serviceKey(s.binding) !== key));
    setConnecting(null);
    refreshWorkspace();
    await probe(family, name, uri, servedModel);
    focusRow(key);
    return null;
  };

  // The downloads: the page's job queue, which the shell follows — the
  // stream's first event is the whole queue, so a download already under way
  // is shown — and this page's own submissions, by benchmark.
  const { jobs, connection } = useJobs();
  const queue = useJobQueue();
  const queueNow = useRef(queue);
  queueNow.current = queue;
  // The queue as it is now, read after an await: never the jobs of the render that started it.
  const jobsNow = useMemo(() => ({ get current(): Jobs { return queueNow.current.jobs(); } }), []);
  const [submissions, setSubmissions] = useState<ReadonlyMap<string, Submission>>(new Map());
  const submitted = useRef(new Set<string>());
  const reloadBenchmarks = benchmarks.reload;

  /**
   * Reads the listing again in place, which says a finished download ready
   * with its digest; once it has, this page's submissions whose job is done
   * are forgotten. A read that fails keeps them — the row still says the
   * digest is being read — and the listing's Retry comes back here.
   */
  const refreshBenchmarks = useCallback(async () => {
    const ok = await reloadBenchmarks();
    if (!ok) return false;
    setSubmissions((all) => new Map([...all].filter(([, s]) => s.kind !== 'accepted' || jobsNow.current.get(s.jobId)?.state.kind !== 'done')));
    return true;
  }, [reloadBenchmarks]);

  /**
   * Downloads that ended done: the listing is read again, and the
   * workspace's counts with it. Focus on a Download that goes with its row's
   * button moves to the section — judged again once the row is gone.
   */
  const finish = useCallback(
    async (names: readonly string[]) => {
      if (names.length === 0) return;
      const focused = document.activeElement?.closest('.rg-setup__action')?.getAttribute('data-benchmark');
      const ok = await refreshBenchmarks();
      refresh.current();
      if (ok && focused !== undefined && focused !== null && names.includes(focused)) setPendingFocus({ benchmarks: true, ifLost: focused });
    },
    [refreshBenchmarks],
  );
  const finishNow = useRef(finish);
  useEffect(() => {
    finishNow.current = finish;
  });

  // The page's job queue, followed by the shell: each event's downloads that ended done.
  useJobEvents((before, after) => {
    void finishNow.current(finishedDownloads(before, after, submitted.current));
  });

  const startDownload = async (name: string) => {
    setSubmissions((all) => new Map(all).set(name, { kind: 'submitting' }));
    const result = await client.post('/benchmarks/{name}/download', undefined, { name });
    if (!result.ok) {
      setSubmissions((all) => new Map(all).set(name, { kind: 'refused', problem: result.problem }));
      return;
    }
    const jobId = result.value.job_id;
    submitted.current.add(jobId);
    setSubmissions((all) => new Map(all).set(name, { kind: 'accepted', jobId }));
    // The stream may have carried the job to its end before this answer came.
    if (jobsNow.current.get(jobId)?.state.kind === 'done') void finish([name]);
  };

  // The download jobs this page asked to cancel, until the stream says each ended.
  const [cancels, setCancels] = useState<ReadonlySet<string>>(new Set());
  const cancelDownload = async (jobId: string) => {
    setCancels((all) => new Set(all).add(jobId));
    const result = await client.del('/jobs/{id}', { id: jobId });
    // Refused — the job ended meanwhile — the stream says how; the button goes back.
    if (!result.ok) setCancels((all) => new Set([...all].filter((id) => id !== jobId)));
  };

  const downloads: Downloads = {
    view: (name) => downloadView(name, submissions.get(name), jobs),
    start: (name) => void startDownload(name),
    cancel: (jobId) => void cancelDownload(jobId),
    cancelling: (jobId) => cancels.has(jobId),
    streamDown: connection === 'disconnected',
  };

  const onImport = async (path: string, name: string) => {
    const result = await client.post('/benchmarks/import', { path, name });
    if (!result.ok) return result.problem;
    benchmarks.replace((list) => [...list.filter((b) => b.name !== result.value.name), result.value]);
    if (mode === 'first') setPendingFocus({ benchmarks: true });
    refreshWorkspace();
    return null;
  };

  const connect = {
    families: loaded?.capabilities.families.map((f) => f.family) ?? [],
    // The first launch's step is a generator's; the same default after it ends, so the family shown never changes by itself.
    initialFamily: 'generator',
    remote: loaded?.capabilities.remote ?? null,
    draft,
    onDraft: setDraft,
    onConnect,
  };

  return (
    <Sheet>
      <h1 ref={heading} tabIndex={-1} className="rg-setup__title">
        Setup
      </h1>
      <p className="rg-setup__intro">Everything a run needs besides its pipeline. Nothing here enters a run’s identity except the benchmarks’ digests.</p>
      <WorkspaceSection state={workspace} onRetry={retryWorkspace} />
      {mode === null ? (
        <div className="rg-setup__pending">
          <Loading label="Reading benchmarks and services" />
        </div>
      ) : mode === 'first' && benchmarks.state.status === 'loaded' && scorable !== null ? (
        <FirstLaunch benchmarks={benchmarks.state.value} scorable={scorable} capabilities={loaded?.capabilities ?? null} onImport={onImport} downloads={downloads} connect={connect} />
      ) : (
        <>
          {endedAt !== undefined && loaded !== null && loaded !== endedAt && services.state.status === 'loaded' ? <NextStep ready={loaded.counts.benchmarks_ready} services={services.state.value} /> : null}
          <Benchmarks
            state={benchmarks.state}
            scorable={scorable}
            stale={benchmarks.stale}
            onRetry={benchmarks.retry}
            onRefresh={() =>
              void refreshBenchmarks().then((ok) => {
                // Its button goes with the inline error it sat in.
                if (ok) setPendingFocus({ benchmarks: true, ifLost: null });
              })
            }
            onImport={onImport}
            downloads={downloads}
            anchor={benchmarksAnchor}
          />
          <Services
            state={services.state}
            onRetry={services.retry}
            connecting={connecting}
            probes={probes}
            testing={testing}
            models={models}
            onServedModel={(key, value) => setModels((m) => new Map(m).set(key, value))}
            onTest={(s) => void probe(s.family, s.name, s.uri, models.get(serviceKey(s)) ?? '')}
            onRemove={onRemove}
            slots={slots}
            refusal={refusal}
            onUndo={onUndo}
            undoId={undoId}
            rowRef={(key) => (el) => {
              if (el === null) rows.current.delete(key);
              else rows.current.set(key, el);
            }}
            slotRef={(key) => (el) => {
              if (el === null) slotEls.current.delete(key);
              else slotEls.current.set(key, el);
            }}
            connect={connect}
            anchor={servicesAnchor}
          />
          <BuildSection state={workspace} onRetry={retryWorkspace} />
        </>
      )}
    </Sheet>
  );
}
