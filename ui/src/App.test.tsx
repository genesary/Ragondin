/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from './api/client.ts';
import { FakeEventSource, installFakeEventSource, mockApi } from './api/testing.ts';
import type { Workspace } from './api/types.ts';
import { App } from './App.tsx';

const BUILD = '0.0.0+aaaaaaaaaaaa';

const WORKSPACE: Workspace = {
  path: '/home/ada/ragondin-ws',
  build: BUILD,
  settings: {
    datasets: '/home/ada/ragondin-ws/datasets',
    services: [
      { family: 'generator', name: 'qwen', uri: 'http://127.0.0.1:50051' },
      { family: 'embedder', name: 'bge', uri: 'http://127.0.0.1:50052' },
    ],
  },
  capabilities: { families: [], remote: true },
};

function show(hash: string, props: { reload?: () => void; eventsPath?: string } = {}) {
  window.history.replaceState(null, '', `/${hash}`);
  const reload = props.reload ?? vi.fn();
  const view = render(<App client={createApiClient()} build={BUILD} reload={reload} {...(props.eventsPath === undefined ? {} : { eventsPath: props.eventsPath })} />);
  return { ...view, reload };
}

const indicator = () => within(screen.getByRole('banner')).getAllByRole('link').find((l) => l.getAttribute('href') === '#setup' && l.closest('nav') === null);
const main = () => screen.getByRole('main');

beforeEach(() => {
  window.sessionStorage.clear();
  window.localStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the shell’s screens', () => {
  it.each([
    ['#runs', 'Runs', 'No runs to show yet'],
    ['#pipeline', 'Pipeline', 'No pipeline chosen'],
    ['#pipeline/hybrid-rrf', 'Pipeline', 'Nothing to show for hybrid-rrf yet'],
    ['#compare', 'Compare', 'No runs chosen to compare'],
    ['#compare/aaa+bbb?baseline=aaa', 'Compare', 'Nothing to show for 2 runs yet'],
    ['#replay', 'Replay', 'No query chosen'],
    ['#replay/aaa/q/1395?with=bbb', 'Replay', 'Nothing to show for query 1395 yet'],
    ['#editor', 'Editor', 'No pipeline open'],
    ['#editor/hybrid-rrf', 'Editor', 'Nothing to show for hybrid-rrf yet'],
    ['#setup', 'Setup', 'No benchmarks or services shown yet'],
  ])('%s renders the %s screen, restored from the hash, in its empty state', async (hash, tab, heading) => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show(hash);
    expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe(tab);
    expect(within(main()).getByRole('heading', { level: 3 }).textContent).toBe(heading);
    const nav = within(screen.getByRole('navigation', { name: 'Screens' }));
    expect(nav.getByRole('link', { name: tab }).getAttribute('aria-current')).toBe('page');
    await screen.findByText(WORKSPACE.path);
  });

  it('lists the six screens, widest to narrowest, each a link to its screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    const links = within(screen.getByRole('navigation', { name: 'Screens' })).getAllByRole('link');
    expect(links.map((l) => [l.textContent, l.getAttribute('href')])).toEqual([
      ['Runs', '#runs'],
      ['Pipeline', '#pipeline'],
      ['Compare', '#compare'],
      ['Replay', '#replay'],
      ['Editor', '#editor'],
      ['Setup', '#setup'],
    ]);
    await screen.findByText(WORKSPACE.path);
  });

  it('gives an empty state the action that leads on, which moves to its screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#pipeline');
    fireEvent.click(within(main()).getByRole('button', { name: 'Open Runs' }));
    await waitFor(() => expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Runs'));
  });

  it('says so, and offers Runs, for an address that names no screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#nowhere');
    expect(within(main()).getByRole('alert').textContent).toContain('#nowhere');
    expect(within(main()).getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe('#runs');
    await screen.findByText(WORKSPACE.path);
  });
});

describe('the workspace indicator', () => {
  it('says the workspace is being read while GET /workspace is in flight', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    expect(within(screen.getByRole('banner')).getByRole('status').textContent).toBe('Reading the workspace');
    await screen.findByText(WORKSPACE.path);
  });

  it('shows the path, the service count and a reachable dot, from GET /workspace', async () => {
    const { requests } = mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    await screen.findByText(WORKSPACE.path);
    const link = indicator();
    expect(link?.textContent).toBe('/home/ada/ragondin-ws, 2 services');
    expect(link?.getAttribute('data-connected')).toBe('true');
    expect(link?.querySelector('.rg-dot')).toBeTruthy();
    expect(requests).toEqual(['GET /api/v1/workspace']);
  });

  it('counts one service in the singular', async () => {
    mockApi({ 'GET /workspace': { body: { ...WORKSPACE, settings: { ...WORKSPACE.settings, services: WORKSPACE.settings.services.slice(0, 1) } } } }, { build: BUILD });
    show('#runs');
    await screen.findByText(WORKSPACE.path);
    expect(indicator()?.textContent).toBe('/home/ada/ragondin-ws, 1 service');
  });

  it('opens Setup when clicked', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    await screen.findByText(WORKSPACE.path);
    const link = indicator();
    if (link === undefined) throw new Error('no indicator');
    fireEvent.click(link);
    await waitFor(() => expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Setup'));
  });

  it('shows the workspace unreachable, in words, and the failure with a retry, when the request fails', async () => {
    const { requests } = mockApi({ 'GET /workspace': [{ network: 'Failed to fetch' }, { body: WORKSPACE }] }, { build: BUILD });
    show('#runs');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('GET /api/v1/workspace');
    expect(alert.textContent).toContain('network_failed');
    expect(indicator()?.textContent).toBe('Workspace unreachable');
    expect(indicator()?.getAttribute('data-connected')).toBe('false');
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));
    await screen.findByText(WORKSPACE.path);
    expect(within(main()).queryByRole('alert')).toBeNull();
    expect(requests).toHaveLength(2);
  });

  it('renders a problem the API answers, with its code and hint', async () => {
    mockApi(
      {
        'GET /workspace': {
          problem: {
            type: 'urn:ragondin:problem:backend_failed',
            title: 'Backend failed',
            status: 500,
            detail: 'The run store could not be read.',
            code: 'backend_failed',
            hint: 'Check the workspace directory, then retry.',
          },
        },
      },
      { build: BUILD },
    );
    show('#runs');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('The run store could not be read.');
    expect(alert.textContent).toContain('Check the workspace directory, then retry.');
    expect(alert.textContent).toContain('backend_failed');
  });
});

describe('the theme control', () => {
  it('sits in the top bar', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    expect(within(screen.getByRole('banner')).getByRole('radiogroup', { name: 'Theme' })).toBeTruthy();
    await screen.findByText(WORKSPACE.path);
  });
});

describe('the build identity handshake', () => {
  it('continues without reloading when the server is this build', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    const { reload } = show('#runs');
    await screen.findByText(WORKSPACE.path);
    expect(reload).not.toHaveBeenCalled();
  });

  it('reloads once when another build answers, then refuses, naming both builds', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: '0.0.0+bbbbbbbbbbbb' });
    const first = show('#runs');
    await waitFor(() => expect(first.reload).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(WORKSPACE.path)).toBeNull();
    first.unmount();

    // The reload fetched the same page again, and the same other build answers.
    const second = show('#runs');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('0.0.0+aaaaaaaaaaaa');
    expect(alert.textContent).toContain('0.0.0+bbbbbbbbbbbb');
    expect(alert.textContent).toContain('build_mismatch');
    expect(second.reload).not.toHaveBeenCalled();
  });
});

describe('the connection state', () => {
  beforeEach(() => {
    installFakeEventSource();
  });

  it('shows the stream connected, then "disconnected — retrying" while it is down, and never current meanwhile', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs', { eventsPath: '/jobs/events' });
    await screen.findByText(WORKSPACE.path);
    const banner = within(screen.getByRole('banner'));
    expect(banner.getByText('connecting')).toBeTruthy();
    act(() => FakeEventSource.latest().open());
    expect(banner.getByText('connected').closest('[data-connected]')?.getAttribute('data-connected')).toBe('true');
    act(() => FakeEventSource.latest().fail());
    expect(banner.getByText('disconnected — retrying').closest('[data-connected]')?.getAttribute('data-connected')).toBe('false');
    expect(banner.queryByText('connected')).toBeNull();
  });

  it('re-checks the build identity when the stream reconnects, and reloads if the server became another build', async () => {
    const { requests } = mockApi({ 'GET /workspace': [{ body: WORKSPACE }, { body: WORKSPACE, build: '0.0.0+cccccccccccc' }] }, { build: BUILD });
    const { reload } = show('#runs', { eventsPath: '/jobs/events' });
    await screen.findByText(WORKSPACE.path);
    act(() => FakeEventSource.latest().open());
    act(() => FakeEventSource.latest().fail());
    act(() => FakeEventSource.latest().open());
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
    expect(requests).toEqual(['GET /api/v1/workspace', 'GET /api/v1/workspace']);
  });

  it('shows no connection state when no stream is open', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#runs');
    await screen.findByText(WORKSPACE.path);
    expect(FakeEventSource.instances).toHaveLength(0);
    expect(screen.queryByText(/connect/)).toBeNull();
  });
});
