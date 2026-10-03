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
import { Services, type Removal, type SessionProbe } from './Services.tsx';
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
  const [removal, setRemoval] = useState<Removal | null>(null);
  const undoId = useId();
  const rows = useRef(new Map<string, HTMLLIElement>());
  const benchmarksAnchor = useRef<HTMLElement>(null);
  const servicesAnchor = useRef<HTMLElement>(null);

  // First launch or the four sections, decided once both listings are read;
  // kept while one is read again, so a Retry never collapses the page.
  const decided = useRef<Mode | null>(null);
  if (benchmarks.state.status === 'loaded' && services.state.status === 'loaded') {
    decided.current = isFirstLaunch(benchmarks.state.value, services.state.value) ? 'first' : 'sections';
  } else if (decided.current === null && (benchmarks.state.status === 'error' || services.state.status === 'error')) {
    decided.current = 'sections';
  }
  const mode = decided.current;

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
  // a row after its probe, or Undo after the row it replaces is gone.
  // Or the Benchmarks section, after an import ends the first launch and takes
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

  const onConnect = async ({ family, name, uri, servedModel }: { family: string; name: string; uri: string; servedModel: string }) => {
    const put = await client.put('/services/{family}/{name}', { uri }, { family, name });
    if (!put.ok) return put.problem;
    const key = serviceKey({ family, name });
    services.replace(put.value.services);
    setModels((m) => new Map(m).set(key, servedModel));
    // A removed binding bound again needs no Undo; any other removal keeps its slot.
    setRemoval((r) => (r !== null && serviceKey(r.binding) === key ? null : r));
    refreshWorkspace();
    await probe(family, name, uri, servedModel);
    focusRow(key);
    return null;
  };

  const onRemove = async (service: ServiceStatus) => {
    const binding: ServiceBinding = { family: service.family, name: service.name, uri: service.uri };
    const key = serviceKey(service);
    // Measured before the row goes, so its slot can hold its place.
    const height = rows.current.get(key)?.getBoundingClientRect().height ?? 0;
    const index = services.state.status === 'loaded' ? services.state.value.findIndex((s) => serviceKey(s) === key) : 0;
    const result = await client.del('/services/{family}/{name}', { family: service.family, name: service.name });
    if (!result.ok) {
      setRemoval({ binding, slot: null, problem: result.problem });
      return;
    }
    services.replace(result.value.services);
    setRemoval({ binding, slot: { index: Math.max(index, 0), height }, problem: null });
    refreshWorkspace();
    setPendingFocus({ undo: true });
  };

  const onUndo = async (binding: ServiceBinding) => {
    const result = await client.put('/services/{family}/{name}', { uri: binding.uri }, { family: binding.family, name: binding.name });
    if (!result.ok) {
      setRemoval((r) => (r === null ? null : { ...r, problem: result.problem }));
      return;
    }
    services.replace(result.value.services);
    setRemoval(null);
    refreshWorkspace();
    focusRow(serviceKey(binding));
  };

  const onImport = async (path: string, name: string) => {
    const result = await client.post('/benchmarks/import', { path, name });
    if (!result.ok) return result.problem;
    benchmarks.replace((list) => [...list.filter((b) => b.name !== result.value.name), result.value]);
    if (decided.current === 'first') setPendingFocus({ benchmarks: true });
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
            probes={probes}
            testing={testing}
            models={models}
            onServedModel={(key, value) => setModels((m) => new Map(m).set(key, value))}
            onTest={(s) => void probe(s.family, s.name, s.uri, models.get(serviceKey(s)) ?? '')}
            onRemove={(s) => void onRemove(s)}
            removal={removal}
            onUndo={(b) => void onUndo(b)}
            undoId={undoId}
            rowRef={(key) => (el) => {
              if (el === null) rows.current.delete(key);
              else rows.current.set(key, el);
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
