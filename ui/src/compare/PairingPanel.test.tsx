/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Comparison, Pairing } from '../api/types.ts';
import { declared } from '../../design/testing/css.ts';
import css from './Compare.css?raw';
import { COMPARISON, HYBRID, RERANK } from './fixtures.ts';
import { PAIR_ROW, PairingPanel, type PairOutcome } from './PairingPanel.tsx';

const WITH_PAIR: Comparison = { ...COMPARISON, pairings: [{ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }] }] };

type OnPair = (pairing: Pairing) => Promise<PairOutcome>;
const KEPT: PairOutcome = { kind: 'kept' };

function panel(comparison: Comparison = COMPARISON, onPair = vi.fn<OnPair>(async () => KEPT)) {
  render(<PairingPanel id="pairing" comparison={comparison} onPair={onPair} />);
  return onPair;
}

const choose = (run: string) => fireEvent.change(screen.getByLabelText('Pair the baseline with'), { target: { value: run } });
const node = (name: string) => screen.getByRole('button', { name });

describe('PairingPanel', () => {
  it('lays the baseline\'s nodes and the other run\'s in two columns', () => {
    panel();
    choose(RERANK);
    expect(within(screen.getByRole('list', { name: 'baseline · dense-only' })).getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual(['dense, baseline']);
    expect(within(screen.getByRole('list', { name: 'B · hybrid-rerank' })).getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual(['bm25, B', 'dense, B', 'rrf, B', 'rerank, B']);
  });

  it('links two nodes by two clicks, either side first', async () => {
    const onPair = panel();
    choose(RERANK);
    fireEvent.click(node('rrf, B'));
    expect(node('rrf, B').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('status').textContent).toBe('rrf picked: choose a node of the baseline to pair it with.');
    // The line is reserved before anything is picked, so picking moves nothing below it.
    expect(declared(css, '.rg-pair__status', 'min-height')).toBe('var(--space-4)');
    await act(async () => fireEvent.click(node('dense, baseline')));
    expect(onPair).toHaveBeenCalledWith({ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rrf' }] });
  });

  it('links by dragging a node onto one of the other column', async () => {
    const onPair = panel();
    choose(HYBRID);
    fireEvent.dragStart(node('dense, baseline'));
    const target = node('rrf, A');
    fireEvent.dragOver(target);
    await act(async () => fireEvent.drop(target));
    expect(onPair).toHaveBeenCalledWith({ pipeline: 'dense-only', other: 'hybrid', pairs: [{ node: 'dense', other: 'rrf' }] });
  });

  it('takes the keyboard: Enter picks, Enter on the other side links, Escape lets go', async () => {
    const onPair = panel();
    choose(RERANK);
    const dense = node('dense, baseline');
    dense.focus();
    fireEvent.keyDown(dense, { key: 'Enter' });
    fireEvent.click(dense);
    expect(dense.getAttribute('aria-pressed')).toBe('true');
    fireEvent.keyDown(dense, { key: 'Escape' });
    expect(dense.getAttribute('aria-pressed')).toBe('false');
    fireEvent.click(dense);
    await act(async () => fireEvent.click(node('rerank, B')));
    expect(onPair).toHaveBeenCalledWith({ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }] });
  });

  it('draws the automatic pairs dashed and the pairs drawn by hand solid', () => {
    panel(WITH_PAIR);
    choose(RERANK);
    const lines = [...document.querySelectorAll<SVGLineElement>('line.rg-pair__link')];
    expect(lines.map((l) => l.dataset.source)).toContain('manual');
    expect(lines.map((l) => l.dataset.source)).toContain('automatic');
    expect(declared(css, '.rg-pair__link[data-source="automatic"]', 'stroke-dasharray')).toBeDefined();
    expect(declared(css, '.rg-pair__link[data-source="manual"]', 'stroke-dasharray')).toBe('none');
    const manual = lines.find((l) => l.dataset.source === 'manual') as SVGLineElement;
    // From the baseline's first row to the other run's fourth.
    expect([Number(manual.getAttribute('y1')), Number(manual.getAttribute('y2'))]).toEqual([PAIR_ROW / 2, PAIR_ROW * 3.5]);
    expect(declared(css, '.rg-pair__node', 'height')).toBe('var(--size-control-l)');
    expect(PAIR_ROW).toBe(40);
  });

  it('removes one pair drawn by hand, and resets them all to automatic', async () => {
    const onPair = panel(WITH_PAIR);
    choose(RERANK);
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Remove the pair dense and rerank' })));
    expect(onPair).toHaveBeenLastCalledWith({ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [] });
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Reset to automatic' })));
    expect(onPair).toHaveBeenLastCalledWith({ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [] });
  });

  it('refuses Reset with its reason while nothing is drawn by hand', () => {
    panel();
    choose(RERANK);
    expect(screen.getByRole('button', { name: 'Reset to automatic' }).getAttribute('aria-disabled')).toBe('true');
  });

  it('says why a run whose pipeline the workspace cannot name pairs automatically only', () => {
    const c: Comparison = { ...COMPARISON, runs: COMPARISON.runs.map((r, i) => (i === 2 ? { ...r, pipeline: null } : r)) };
    panel(c);
    choose(RERANK);
    expect(screen.getByText(/matches no pipeline document of the workspace, or several: it pairs automatically only/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'rerank, B' })).toBeNull();
  });

  it('shows the API\'s refusal inline', async () => {
    const onPair = vi.fn<OnPair>(async () => ({ kind: 'refused', problem: { code: 'request_invalid', message: 'node rerank is paired twice', hint: 'Pair each node once.', location: null, status: 400 } }));
    panel(COMPARISON, onPair);
    choose(RERANK);
    fireEvent.click(node('dense, baseline'));
    await act(async () => fireEvent.click(node('rerank, B')));
    expect(screen.getByRole('alert').textContent).toContain('node rerank is paired twice');
  });

  it('says what was kept in its status line once the API keeps it', async () => {
    panel();
    choose(RERANK);
    fireEvent.click(node('dense, baseline'));
    await act(async () => fireEvent.click(node('rerank, B')));
    expect(screen.getByRole('status').textContent).toBe('dense paired with rerank; 1 pair by hand.');
  });

  it('says so in its status line when reset to automatic', async () => {
    panel(WITH_PAIR);
    choose(RERANK);
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Reset to automatic' })));
    expect(screen.getByRole('status').textContent).toBe('Reset to automatic.');
  });

  it('is busy while a pair is kept, and refuses a second link in words rather than dropping it', async () => {
    let release: (o: PairOutcome) => void = () => {};
    const onPair = panel(COMPARISON, vi.fn<OnPair>(() => new Promise((r) => (release = r))));
    choose(RERANK);
    fireEvent.click(node('dense, baseline'));
    fireEvent.click(node('rerank, B'));
    const region = screen.getByRole('region', { name: 'Pair nodes' });
    expect(region.getAttribute('aria-busy')).toBe('true');
    expect(screen.getByRole('status').textContent).toBe('Keeping dense paired with rerank…');
    fireEvent.click(node('dense, baseline'));
    fireEvent.click(node('rrf, B'));
    expect(screen.getByRole('status').textContent).toBe('Still keeping the last pair: link again once it is kept.');
    expect(onPair).toHaveBeenCalledTimes(1);
    await act(async () => release(KEPT));
    expect(region.getAttribute('aria-busy')).toBeNull();
  });

  it('says nothing of an answer a newer comparison overtook', async () => {
    panel(COMPARISON, vi.fn<OnPair>(async () => ({ kind: 'superseded' })));
    choose(RERANK);
    fireEvent.click(node('dense, baseline'));
    await act(async () => fireEvent.click(node('rerank, B')));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('status').textContent).toBe('');
  });

  it('keeps the list of pairs drawn by hand in place, its height reserved, before any is drawn', () => {
    panel();
    choose(RERANK);
    const list = screen.getByRole('list', { name: 'Pairs drawn by hand' });
    expect(list.textContent).toBe('None yet.');
    expect(declared(css, '.rg-pair__pairs', 'min-height')).toBe('var(--size-control-s)');
  });
});
