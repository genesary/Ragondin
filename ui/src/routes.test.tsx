/** @vitest-environment happy-dom */
import { act, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { formatHash, navigate, parseHash, useRoute, type Route } from './routes.ts';

/** Renders what the router reads from the address, as a screen receives it. */
function Probe() {
  return <output>{JSON.stringify(useRoute())}</output>;
}

/** Loads the page at `hash` and returns the route the shell would render. */
function load(hash: string): Route | null {
  window.history.replaceState(null, '', `/${hash}`);
  render(<Probe />);
  return JSON.parse(screen.getByRole('status').textContent ?? 'null');
}

beforeEach(() => {
  window.history.replaceState(null, '', '/');
});

describe('#runs', () => {
  it('has no state, and restores from the hash on load', () => {
    expect(formatHash({ screen: 'runs' })).toBe('#runs');
    expect(load('#runs')).toEqual({ screen: 'runs' });
  });

  it('is what an empty hash shows', () => {
    expect(load('')).toEqual({ screen: 'runs' });
    expect(parseHash('#')).toEqual({ screen: 'runs' });
  });
});

describe('#runs?sel=<id>,<id>…', () => {
  it('carries the selection in order, joined by commas, and restores it from the hash on load', () => {
    const route: Route = { screen: 'runs', sel: ['bbb', 'aaa'] };
    expect(formatHash(route)).toBe('#runs?sel=bbb,aaa');
    expect(load('#runs?sel=bbb,aaa')).toEqual(route);
  });

  it('leaves the selection out when it is empty', () => {
    expect(formatHash({ screen: 'runs', sel: [] })).toBe('#runs');
  });

  it('encodes an id holding the separator, so it round-trips', () => {
    const route: Route = { screen: 'runs', sel: ['a,b', 'c'] };
    expect(formatHash(route)).toBe('#runs?sel=a%2Cb,c');
    expect(parseHash(formatHash(route))).toEqual(route);
  });
});

describe('#pipeline', () => {
  it('is the screen before a pipeline is chosen', () => {
    expect(formatHash({ screen: 'pipeline' })).toBe('#pipeline');
    expect(load('#pipeline')).toEqual({ screen: 'pipeline' });
  });
});

describe('#pipeline/<name>', () => {
  it('carries the pipeline’s name, and restores it from the hash on load', () => {
    expect(formatHash({ screen: 'pipeline', name: 'hybrid-rrf' })).toBe('#pipeline/hybrid-rrf');
    expect(load('#pipeline/hybrid-rrf')).toEqual({ screen: 'pipeline', name: 'hybrid-rrf' });
  });
});

describe('#compare', () => {
  it('is the screen before runs are chosen', () => {
    expect(formatHash({ screen: 'compare', ids: [] })).toBe('#compare');
    expect(load('#compare')).toEqual({ screen: 'compare', ids: [] });
  });
});

describe('#compare/<ids>?baseline=<id>', () => {
  it('carries the runs, joined by +, and the baseline, and restores them from the hash on load', () => {
    const route: Route = { screen: 'compare', ids: ['aaa', 'bbb', 'ccc'], baseline: 'bbb' };
    expect(formatHash(route)).toBe('#compare/aaa+bbb+ccc?baseline=bbb');
    expect(load('#compare/aaa+bbb+ccc?baseline=bbb')).toEqual(route);
  });

  it('leaves the baseline out when there is none', () => {
    expect(formatHash({ screen: 'compare', ids: ['aaa', 'bbb'] })).toBe('#compare/aaa+bbb');
    expect(load('#compare/aaa+bbb')).toEqual({ screen: 'compare', ids: ['aaa', 'bbb'] });
  });

  it('encodes an id holding the separator, so it round-trips', () => {
    const route: Route = { screen: 'compare', ids: ['a+b', 'c'] };
    expect(formatHash(route)).toBe('#compare/a%2Bb+c');
    expect(parseHash(formatHash(route))).toEqual(route);
  });

  it('is no route without a run', () => {
    expect(parseHash('#compare/')).toBeNull();
    expect(parseHash('#compare?baseline=aaa')).toBeNull();
    expect(parseHash('#compare/a++b')).toBeNull();
  });
});

describe('#replay', () => {
  it('is the screen before a query is chosen', () => {
    expect(formatHash({ screen: 'replay' })).toBe('#replay');
    expect(load('#replay')).toEqual({ screen: 'replay' });
  });
});

describe('#replay/<run>', () => {
  it('carries the run before a query is chosen, and restores it from the hash on load', () => {
    expect(formatHash({ screen: 'replay', run: 'aaa' })).toBe('#replay/aaa');
    expect(load('#replay/aaa')).toEqual({ screen: 'replay', run: 'aaa' });
  });
});

describe('#replay/<run>/q/<query>?with=<run>', () => {
  it('carries the run, the query and the run beside it, and restores them from the hash on load', () => {
    const route: Route = { screen: 'replay', run: 'aaa', query: '1395', with: 'bbb' };
    expect(formatHash(route)).toBe('#replay/aaa/q/1395?with=bbb');
    expect(load('#replay/aaa/q/1395?with=bbb')).toEqual(route);
  });

  it('is a single replay without `with`', () => {
    expect(formatHash({ screen: 'replay', run: 'aaa', query: '1395' })).toBe('#replay/aaa/q/1395');
    expect(load('#replay/aaa/q/1395')).toEqual({ screen: 'replay', run: 'aaa', query: '1395' });
  });

  it('encodes a query id holding a slash, so it round-trips', () => {
    const route: Route = { screen: 'replay', run: 'aaa', query: 'test/12' };
    expect(formatHash(route)).toBe('#replay/aaa/q/test%2F12');
    expect(parseHash(formatHash(route))).toEqual(route);
  });
});

describe('#editor', () => {
  it('is the screen before a pipeline is opened', () => {
    expect(formatHash({ screen: 'editor' })).toBe('#editor');
    expect(load('#editor')).toEqual({ screen: 'editor' });
  });
});

describe('#editor/<name>', () => {
  it('carries the pipeline’s name, and restores it from the hash on load', () => {
    expect(formatHash({ screen: 'editor', name: 'hybrid-rrf' })).toBe('#editor/hybrid-rrf');
    expect(load('#editor/hybrid-rrf')).toEqual({ screen: 'editor', name: 'hybrid-rrf' });
  });
});

describe('#setup', () => {
  it('has no state, and restores from the hash on load', () => {
    expect(formatHash({ screen: 'setup' })).toBe('#setup');
    expect(load('#setup')).toEqual({ screen: 'setup' });
  });
});

describe('the address', () => {
  it.each([
    '#nowhere',
    '#pipeline/',
    '#pipeline/a/b',
    '#replay/aaa/1395',
    '#replay/aaa?with=bbb',
    '#replay?with=bbb',
    '#runs/extra',
    '#runs?sel=',
    '#runs?sel=aaa,,bbb',
    '#runs?sel=aaa,..',
    '#editor/%E0%A4%A',
    // A value of `.` or `..`, typed or escaped, would name another API path.
    '#pipeline/..',
    '#editor/%2E%2E',
    '#replay/%2e/q/1',
    '#replay/aaa/q/..',
    '#compare/aaa+..',
    // An empty baseline or companion run is no value.
    '#compare/aaa+bbb?baseline=',
    '#replay/aaa/q/1395?with=',
  ])(
    'reads %s as no route rather than a guess',
    (hash) => {
      expect(parseHash(hash)).toBeNull();
    },
  );

  it('is followed when it changes after load', async () => {
    load('#runs');
    act(() => navigate({ screen: 'editor', name: 'starter' }));
    expect(window.location.hash).toBe('#editor/starter');
    // The browser announces a hash change after the fact, as an event.
    await waitFor(() => expect(JSON.parse(screen.getByRole('status').textContent ?? 'null')).toEqual({ screen: 'editor', name: 'starter' }));
  });

  it('replaces the current history entry when asked, so Back skips it', async () => {
    load('#runs');
    act(() => navigate({ screen: 'setup' }));
    await waitFor(() => expect(window.location.hash).toBe('#setup'));
    const length = window.history.length;
    act(() => navigate({ screen: 'editor', name: 'starter' }, { replace: true }));
    expect(window.location.hash).toBe('#editor/starter');
    expect(window.history.length).toBe(length);
    await waitFor(() => expect(JSON.parse(screen.getByRole('status').textContent ?? 'null')).toEqual({ screen: 'editor', name: 'starter' }));
  });

  it('is followed when the user edits it', () => {
    load('#runs');
    act(() => {
      window.location.hash = '#setup';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    expect(JSON.parse(screen.getByRole('status').textContent ?? 'null')).toEqual({ screen: 'setup' });
  });
});
