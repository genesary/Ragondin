// The Setup screen: everything a run needs besides its pipeline (the
// front-end design, § 3 and § 6) — the workspace, the benchmarks, the
// services, and what this build can run — or, on a workspace with no
// benchmark and no service, the first-launch invitation. ARCHITECTURE.md
// § The Setup screen.
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry, ServiceBinding, ServiceStatus, Workspace } from '../api/types.ts';
import type { SetupSection } from '../routes.ts';
import { Loading, type RequestState } from '../shell/states.tsx';
import { Benchmarks } from './Benchmarks.tsx';
import { FirstLaunch } from './FirstLaunch.tsx';
import { isFirstLaunch, serviceKey } from './model.ts';
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
};

type Mode = 'first' | 'sections';

/**
 * How long a removed binding can be brought back by Undo before the removal
 * is written. Nothing is written meanwhile, so Undo restores the binding, its
 * place in `workspace.toml` and its comments exactly.
 */
export const UNDO_WINDOW_MS = 10_000;

/**
 * Reads a listing, keeping only the answer to the last read asked for, and
 * lets a write replace it in place.
 */
function useListing<T>(read: () => Promise<{ ok: true; value: T } | { ok: false; problem: ApiProblem }>) {
  const [state, setState] = useState<RequestState<T>>({ status: 'loading' });
  const latest = useRef(0);
  const load = useCallback(async () => {
    const mine = ++latest.current;
    const result = await read();
    if (mine !== latest.current) return;
    setState(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [read]);
  useEffect(() => {
    void load();
  }, [load]);
  const retry = () => {
    setState({ status: 'loading' });
    void load();
  };
  /** A write's answer replaces the listing, and overtakes any read still in flight. */
  const replace = useCallback((value: T | ((prev: T) => T)) => {
    latest.current += 1;
    setState((prev) => {
      if (typeof value !== 'function') return { status: 'loaded', value };
      return prev.status === 'loaded' ? { status: 'loaded', value: (value as (prev: T) => T)(prev.value) } : prev;
    });
  }, []);
  return { state, retry, replace };
}

export function SetupScreen({ client, workspace, refreshWorkspace, retryWorkspace, section }: SetupScreenProps) {
  const readBenchmarks = useCallback(async () => {
    const r = await client.get('/benchmarks');
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
  const next: Mode | null =
    benchmarks.state.status === 'loaded' && services.state.status === 'loaded'
      ? isFirstLaunch(benchmarks.state.value, services.state.value)
        ? 'first'
        : 'sections'
      : mode === null && (benchmarks.state.status === 'error' || services.state.status === 'error')
        ? 'sections'
        : mode;
  if (next !== mode) setMode(next);

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
  const [pendingFocus, setPendingFocus] = useState<{ row: string } | { undo: true } | { benchmarks: true } | null>(null);
  useEffect(() => {
    if (pendingFocus === null) return;
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

  const onImport = async (path: string, name: string) => {
    const result = await client.post('/benchmarks/import', { path, name });
    if (!result.ok) return result.problem;
    benchmarks.replace((list) => [...list.filter((b) => b.name !== result.value.name), result.value]);
    if (mode === 'first') setPendingFocus({ benchmarks: true });
    refreshWorkspace();
    return null;
  };

  const loaded = workspace.status === 'loaded' ? workspace.value : null;
  const connect = {
    families: loaded?.capabilities.families.map((f) => f.family) ?? [],
    remote: loaded?.capabilities.remote ?? null,
    onConnect,
  };

  return (
    <Sheet>
      <p className="rg-setup__intro">Everything a run needs besides its pipeline. Nothing here enters a run’s identity except the benchmarks’ digests.</p>
      <WorkspaceSection state={workspace} onRetry={retryWorkspace} />
      {mode === null ? (
        <div className="rg-setup__pending">
          <Loading label="Reading benchmarks and services" />
        </div>
      ) : mode === 'first' && benchmarks.state.status === 'loaded' ? (
        <FirstLaunch benchmarks={benchmarks.state.value} capabilities={loaded?.capabilities ?? null} onImport={onImport} connect={{ ...connect, initialFamily: 'generator' }} />
      ) : (
        <>
          <Benchmarks state={benchmarks.state} onRetry={benchmarks.retry} onImport={onImport} anchor={benchmarksAnchor} />
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
