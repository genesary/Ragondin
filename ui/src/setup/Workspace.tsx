// The two sections read from `GET /workspace`, as the shell read it: the
// Workspace — where it is, where benchmarks are read from, what it holds —
// and This build — what the binary answering can run on its own.
import { Section, Table } from '../../design/index.ts';
import type { Workspace } from '../api/types.ts';
import { ErrorState, Resource, type RequestState } from '../shell/states.tsx';
import { splitBuild } from './model.ts';

const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;

type Props = { state: RequestState<Workspace>; onRetry: () => void };

export function WorkspaceSection({ state, onRetry }: Props) {
  return (
    <Section heading="Workspace">
      <Resource state={state} loading="Reading the workspace" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(workspace) => (
          <>
            <dl className="rg-setup__facts">
              <dt>Root</dt>
              <dd>
                <code>{workspace.path}</code>
              </dd>
              <dt>Datasets directory</dt>
              <dd>
                <code>{workspace.settings.datasets}</code>
              </dd>
              <dt>Holds</dt>
              <dd>
                {[
                  count(workspace.counts.runs, 'run'),
                  count(workspace.counts.pipelines, 'pipeline'),
                  count(workspace.counts.benchmarks_ready, 'benchmark ready', 'benchmarks ready'),
                  count(workspace.counts.services_connected, 'service connected', 'services connected'),
                ].join(' · ')}
              </dd>
            </dl>
            <p className="rg-setup__note">
              To work in another workspace, start <code>ragondin ui --workspace &lt;dir&gt;</code> on its root; a directory that is not a workspace yet is made one.
            </p>
          </>
        )}
      </Resource>
    </Section>
  );
}

export function BuildSection({ state, onRetry }: Props) {
  return (
    <Section heading="This build" caption="What the binary answering can run on its own. It is not a setting.">
      <Resource state={state} loading="Reading the workspace" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(workspace) => {
          const { version, commit } = splitBuild(workspace.build);
          return (
            <>
              <dl className="rg-setup__facts">
                <dt>Version</dt>
                <dd>{version}</dd>
                {commit === null ? null : (
                  <>
                    <dt>Commit</dt>
                    <dd>
                      <code>{commit}</code>
                    </dd>
                  </>
                )}
                <dt>Remote components</dt>
                <dd>{workspace.capabilities.remote ? 'on' : 'off'}</dd>
              </dl>
              <p className="rg-setup__note">
                {workspace.capabilities.remote ? 'This build can call a service bound in the workspace.' : 'This build cannot call a service: it lacks the remote feature.'} The page and the server are the same build: the shell checks it whenever the page loads.
              </p>
              <Table
                caption="Local implementations per family"
                columns={[
                  { id: 'family', label: 'Family' },
                  { id: 'local', label: 'Local implementations' },
                ]}
                rows={workspace.capabilities.families.map((f) => ({
                  id: f.family,
                  cells: [f.family, f.local.length === 0 ? <span className="rg-setup__absent">none in this build</span> : f.local.join(', ')],
                }))}
              />
              <p className="rg-setup__note">Adding a Local component is a rebuild, not a setting.</p>
            </>
          );
        }}
      </Resource>
    </Section>
  );
}
