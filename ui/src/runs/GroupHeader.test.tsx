/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { RequestState } from '../shell/states.tsx';
import { GroupHeader } from './GroupHeader.tsx';
import type { RunGroup, RunRow, ShapeNode } from './model.ts';

const HASH = '821bafbd3fa0'.padEnd(64, '7');

const row = (id: string): RunRow => ({
  id,
  pipeline: HASH,
  pipelineName: null,
  benchmark: 'd',
  benchmarkName: null,
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
});

const group = (over: Partial<RunGroup> = {}): RunGroup => ({ key: 'hybrid', name: 'hybrid', pipeline: HASH, shapeFrom: '1', rows: [row('1'), row('2')], ...over });

const SHAPE: ShapeNode[] = [
  { node: 'bm25', family: 'retriever' },
  { node: 'dense', family: 'retriever' },
  { node: 'rrf', family: 'fusion' },
  { node: 'rerank', family: 'reranker' },
  { node: 'x', family: null, word: 'extension' },
];

function show(g: RunGroup, shape: RequestState<ShapeNode[]> = { status: 'loaded', value: SHAPE }) {
  render(
    <table>
      <tbody>
        <GroupHeader group={g} shape={shape} columns={6} />
      </tbody>
    </table>,
  );
}

describe('the group header', () => {
  it('is a row-group header across every column', () => {
    show(group());
    const th = screen.getByRole('rowheader');
    expect(th.getAttribute('scope')).toBe('rowgroup');
    expect(th.getAttribute('colspan')).toBe('6');
  });

  it('names the pipeline as a link to its Pipeline screen', () => {
    show(group());
    expect(screen.getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
  });

  it('names a pipeline without a name by its short hash, linking by the full one', () => {
    show(group({ key: HASH, name: null }));
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

  it('counts its runs', () => {
    show(group());
    expect(screen.getByText('2 runs')).toBeTruthy();
  });

  it('counts one run in the singular', () => {
    show(group({ rows: [row('1')] }));
    expect(screen.getByText('1 run')).toBeTruthy();
  });

  it('says so while the shape is being read', () => {
    show(group(), { status: 'loading' });
    expect(screen.getByRole('status').textContent).toBe('Reading the shape');
  });

  it('says what failed when the shape could not be read', () => {
    show(group(), { status: 'error', problem: { code: 'run_unreadable', message: 'GET /api/v1/runs/1 answered 500.', hint: 'h', location: null, status: 500 } });
    expect(screen.getByText('Shape not read: GET /api/v1/runs/1 answered 500.')).toBeTruthy();
  });
});
