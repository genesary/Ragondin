import type { Workspace } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { Resource, type RequestState } from './states.tsx';

const count = (n: number, one: string) => `${n.toLocaleString('en-US')} ${one}${n === 1 ? '' : 's'}`;

/**
 * The workspace, always visible in the top bar (the front-end design, § 3):
 * its path, how many services it binds and whether it answered — a filled
 * dot, or a hollow one and the word — as a link that opens Setup. It says the
 * workspace is being read while `GET /workspace` is in flight; the failure
 * itself is rendered by the shell, where there is room for it.
 */
export function WorkspaceIndicator({ state }: { state: RequestState<Workspace> }) {
  const setup = formatHash({ screen: 'setup' });
  return (
    <Resource
      state={state}
      loading="Reading the workspace"
      error={() => (
        <a className="rg-workspace" href={setup} data-connected="false">
          <span className="rg-dot" aria-hidden="true" />
          Workspace unreachable
        </a>
      )}
    >
      {(workspace) => (
        <a className="rg-workspace" href={setup} data-connected="true">
          <span className="rg-dot" aria-hidden="true" />
          <span>{workspace.path}</span>, {count(workspace.settings.services.length, 'service')}
        </a>
      )}
    </Resource>
  );
}
