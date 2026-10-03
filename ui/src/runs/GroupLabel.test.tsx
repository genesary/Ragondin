/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Table } from '../../design/index.ts';
import { GroupLabel } from './GroupLabel.tsx';
import type { RunGroup, RunRow, ShapeNode } from './model.ts';

const HASH = '821bafbd3fa0'.padEnd(64, '7');

const row = (id: string): RunRow => ({
  source: { kind: 'run', id },
  pipeline: HASH,
  pipelineNames: [],
  launchedAs: null,
  launchedHeld: null,
  launchRecorded: false,
  benchmark: 'd',
  benchmarkNames: [],
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
});

const group = (over: Partial<RunGroup> = {}): RunGroup => ({ key: 'name:hybrid', names: ['hybrid'], held: 'exactly', pipeline: HASH, shapeKey: HASH, rows: [row('1'), row('2')], ...over });

const SHAPE: ShapeNode[] = [
  { node: 'bm25', family: 'retriever' },
  { node: 'dense', family: 'retriever' },
  { node: 'rrf', family: 'fusion' },
  { node: 'rerank', family: 'reranker' },
  { node: 'x', family: null, word: 'extension' },
];

function show(g: RunGroup, shape: ShapeNode[] | null = SHAPE) {
  render(<Table caption="runs" columns={[{ id: 'a', label: 'A' }]} rows={[{ kind: 'group', id: g.key, label: <GroupLabel group={g} shape={shape} /> }]} />);
}

describe('the group heading', () => {
  it('names the pipeline as a link to its Pipeline screen', () => {
    show(group());
    expect(screen.getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
  });

  it('names every document that is the pipeline, each a link to its own Pipeline screen', () => {
    show(group({ names: ['hybrid', 'hybrid-copy'] }));
    expect(screen.getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
    expect(screen.getByRole('link', { name: 'hybrid-copy' }).getAttribute('href')).toBe('#pipeline/hybrid-copy');
  });

  it('separates the names, so the heading reads as a list and not as one run-together word', () => {
    show(group({ names: ['hybrid', 'hybrid-copy', 'hybrid-old'] }));
    const header = document.querySelector('th[scope="rowgroup"]') as HTMLElement;
    expect(header.textContent).toContain('hybrid, hybrid-copy, hybrid-old');
    // Only between names: none before the first, none after the last.
    expect(header.textContent?.startsWith('hybrid')).toBe(true);
    expect(header.textContent).not.toContain('hybrid-old,');
  });

  it('does not link a recorded name the workspace no longer holds, and says why', () => {
    show(group({ names: ['hybrid-old'], held: 'gone' }));
    expect(screen.queryByRole('link')).toBeNull();
    const header = document.querySelector('th[scope="rowgroup"]') as HTMLElement;
    expect(header.textContent).toContain('hybrid-old');
    expect(header.textContent).toContain('no longer a document in this workspace');
  });

  it('does not link a recorded name held only under another case, and says so apart from a gone one', () => {
    show(group({ names: ['Hybrid'], held: 'other_case' }));
    expect(screen.queryByRole('link')).toBeNull();
    const header = document.querySelector('th[scope="rowgroup"]') as HTMLElement;
    expect(header.textContent).toContain('Hybrid');
    expect(header.textContent).toContain('held only under another case');
    expect(header.textContent).not.toContain('no longer');
  });

  it('does not link a recorded name nothing checked, and says so', () => {
    show(group({ names: ['hybrid'], held: 'unchecked' }));
    expect(screen.queryByRole('link')).toBeNull();
    expect(document.querySelector('th[scope="rowgroup"]')?.textContent).toContain('not checked against the workspace');
  });

  it('links a recorded name the workspace holds exactly', () => {
    show(group({ names: ['hybrid'], held: 'exactly' }));
    expect(screen.getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
    expect(document.querySelector('th[scope="rowgroup"]')?.textContent).not.toMatch(/no longer|another case/);
  });

  it('names a pipeline without a name by its short hash, linking by the full one', () => {
    // The group's key is not a pipeline's address: the hash is.
    show(group({ key: `hash:${HASH}`, names: [] }));
    expect(screen.getByRole('link', { name: 'pipeline 821bafbd3fa0' }).getAttribute('href')).toBe(`#pipeline/${HASH}`);
  });

  it('draws the shape as family tiles in pipeline order, each with its glyph named', () => {
    show(group());
    const tiles = [...document.querySelectorAll('.rg-tile')];
    expect(tiles.map((t) => t.getAttribute('data-family'))).toEqual(['retriever', 'retriever', 'fusion', 'reranker']);
    expect(tiles.every((t) => t.querySelector('svg[role="img"]') !== null)).toBe(true);
    expect(screen.getByRole('list', { name: 'Shape' }).children).toHaveLength(5);
  });

  it('writes a family the design system draws no tile for as its word', () => {
    show(group());
    expect(screen.getByText('extension')).toBeTruthy();
  });

  it('counts its runs, one in the singular', () => {
    show(group());
    expect(screen.getByText('2 runs')).toBeTruthy();
  });

  it('counts one run in the singular', () => {
    show(group({ rows: [row('1')] }));
    expect(screen.getByText('1 run')).toBeTruthy();
  });

  it('draws no shape when the listing carries none for the group', () => {
    show(group({ shapeKey: null }), null);
    expect(screen.queryByRole('list', { name: 'Shape' })).toBeNull();
    expect(screen.getByText('2 runs')).toBeTruthy();
  });
});
