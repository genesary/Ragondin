// The first-launch state: on a workspace with no benchmark on disk and no
// service, Setup is a two-step invitation rather than four sections (the
// front-end design, § 3: the default path, made the obvious one).
import { Section } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry, Capabilities } from '../api/types.ts';
import { ActionSlot, DownloadButton, downloadWords, isLive, Licence, type Downloads } from './Benchmarks.tsx';
import { ConnectForm, ImportForm, type ConnectFormProps } from './forms.tsx';
import { formatSize, smallestAvailable } from './model.ts';

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
  capabilities: Capabilities | null;
  onImport: (path: string, name: string) => Promise<ApiProblem | null>;
  downloads: Downloads;
  connect: ConnectFormProps;
};

export function FirstLaunch({ benchmarks, capabilities, onImport, downloads, connect }: FirstLaunchProps) {
  const first = smallestAvailable(benchmarks);
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
                — the smallest, a good first run.
              </p>
              <div className="rg-setup__submit">
                <ActionSlot benchmark={first.name}>
                  <DownloadButton view={view} onStart={() => downloads.start(first.name)} />
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
