// The first-launch state: on a workspace with no benchmark on disk and no
// service, Setup is a two-step invitation rather than four sections (the
// front-end design, § 3: the default path, made the obvious one).
import { ButtonLink, Section } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry, Capabilities, Scorable, ServiceStatus } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { ActionSlot, DownloadControls, downloadWords, isLive, Licence, type Downloads } from './Benchmarks.tsx';
import { ConnectForm, ImportForm, type ConnectFormProps } from './forms.tsx';
import { firstBenchmark, formatSize, serviceKey } from './model.ts';

/**
 * Why a retrieval-only pipeline needs no service, from what this build
 * carries: its built-in retrievers and rerankers, or nothing claimed when it
 * has none.
 */
export function builtInSentence(capabilities: Capabilities | null): string | null {
  const has = (family: string) => (capabilities?.families.find((f) => f.family === family)?.parameters.length ?? 0) > 0;
  const parts = [has('retriever') ? 'retrievers' : null, has('reranker') ? 'a reranker' : null].filter((p) => p !== null);
  if (parts.length === 0) return null;
  return `A retrieval-only pipeline needs no service: this build has ${parts.join(' and ')} built in.`;
}

export type FirstLaunchProps = {
  benchmarks: readonly BenchmarkEntry[];
  /** The listing's `scorable`, which the recommendation reads. */
  scorable: Scorable;
  capabilities: Capabilities | null;
  onImport: (path: string, name: string) => Promise<ApiProblem | null>;
  downloads: Downloads;
  connect: ConnectFormProps;
};

export function FirstLaunch({ benchmarks, scorable, capabilities, onImport, downloads, connect }: FirstLaunchProps) {
  const pick = firstBenchmark(benchmarks, scorable);
  const first = pick?.entry ?? null;
  const view = first === null ? null : downloads.view(first.name);
  const builtIn = builtInSentence(capabilities);
  return (
    <Section heading="Get started" caption="This workspace holds no benchmark and binds no service yet.">
      <ol className="rg-setup__steps">
        <li className="rg-setup__step">
          <h3 className="rg-setup__subheading">Add a benchmark</h3>
          {first === null || view === null ? null : (
            <>
              <p className="rg-setup__lead">
                <span className="rg-setup__name">{first.name}</span>, {formatSize(first.state.size_bytes)}
                {first.licence === null ? null : (
                  <>
                    , <Licence entry={first} />
                  </>
                )}{' '}
                {pick?.why === 'first-run' ? <> — the first-run benchmark: the Editor’s example pipeline, retrieval-only, is scored on it with no service.</> : <> — the smallest, a good first run.</>}
              </p>
              <div className="rg-setup__submit">
                <ActionSlot benchmark={first.name}>
                  <DownloadControls benchmark={first.name} view={view} downloads={downloads} />
                </ActionSlot>
                {/* The lead above already gives the size: an idle line says nothing. */}
                <span className="rg-setup__said">
                  {view.kind === 'idle' ? null : downloadWords(view, first.state.size_bytes, downloads.streamDown)}
                  {/* Its words already say "last known"; the line stays short enough not to wrap. */}
                  {downloads.streamDown && isLive(view) ? <> — disconnected, retrying</> : null}
                </span>
              </div>
              <p className="rg-setup__lead">Or import a corpus you hold:</p>
            </>
          )}
          <ImportForm onImport={onImport} />
        </li>
        <li className="rg-setup__step">
          <h3 className="rg-setup__subheading">Connect a generator, if your pipeline ends in an answer</h3>
          {builtIn === null ? null : <p className="rg-setup__lead">{builtIn}</p>}
          <ConnectForm {...connect} />
        </li>
      </ol>
    </Section>
  );
}

/**
 * What the first launch leaves behind once it ends, so the way on is not
 * lost with it: what is now in place, and the next step — a pipeline in the
 * Editor, launched from Runs — or, while no benchmark is ready, a benchmark.
 * `ready` is the workspace's count (`WorkspaceCounts.benchmarks_ready`), read
 * after the write that ended the first launch.
 */
export function NextStep({ ready, services }: { ready: number; services: readonly ServiceStatus[] }) {
  if (ready === 0) {
    const bound = services.length === 0 ? 'No benchmark is ready yet.' : `${services.map(serviceKey).join(', ')} ${services.length === 1 ? 'is' : 'are'} bound.`;
    return (
      <Section heading="Next step">
        <p className="rg-setup__lead">{bound} Next: add a benchmark below — a run is one pipeline on one benchmark.</p>
      </Section>
    );
  }
  return (
    <Section heading="Next step">
      <p className="rg-setup__lead">
        {ready} {ready === 1 ? 'benchmark is' : 'benchmarks are'} ready; the table below says which pipelines each scores. Next: build a pipeline in the Editor, then launch it from{' '}
        <a className="rg-setup__link" href={formatHash({ screen: 'runs' })}>
          Runs
        </a>
        .
      </p>
      <div className="rg-setup__submit">
        <ButtonLink kind="primary" href={formatHash({ screen: 'editor' })}>
          Open Editor
        </ButtonLink>
      </div>
    </Section>
  );
}
