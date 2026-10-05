import { StatusDot } from '../../design/index.ts';
import type { Workspace } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { Resource, type RequestState } from './states.tsx';

const count = (n: number, one: string) => `${n.toLocaleString('en-US')} ${one}${n === 1 ? '' : 's'}`;

/**
 * The workspace, always visible in the top bar (the front-end design, § 3):
 * its path, how many services it binds and whether it answered — a filled
 * dot, or a hollow ring and the words — as a link that opens Setup. It says
 * the workspace is being read while `GET /workspace` is in flight; the failure
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
          <StatusDot connected={false} />
          Workspace unreachable
        </a>
      )}
    >
      {(workspace) => (
        <a className="rg-workspace" href={setup} data-connected="true">
          <StatusDot connected />
          {/* Cut at its start when its line is too narrow, so the workspace's own folder stays in view; the whole path
              is the link's name and the title. */}
          <span className="rg-workspace__path" title={workspace.path}>
            <bdi>{workspace.path}</bdi>
          </span>, {count(workspace.settings.services.length, 'service')}
        </a>
      )}
    </Resource>
  );
}
