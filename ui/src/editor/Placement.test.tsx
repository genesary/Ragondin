/** @vitest-environment happy-dom */
// Where a placed node lands — in free space, beside the selected node, never
// on a card — and the "Tidy layout" action, one undo step.
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi } from '../api/testing.ts';
import { resolveLayout, toModel } from '../canvas/index.ts';
import { NODE_HEIGHT, NODE_WIDTH } from '../canvas/layout.ts';
import { toGraph, type WireDocument } from './document.ts';
import { Editor } from './Editor.tsx';
import { GRAMMAR, HYBRID, SERVICES, WORKSPACE } from './fixtures.ts';
import type { EditorLayout } from './store.ts';

// Every node of the hybrid stacked on one spot: a layout only a person dragging carelessly would leave.
const PILED: EditorLayout = { question: { x: 0, y: 0 }, lexical: { x: 288, y: 0 }, vectors: { x: 288, y: 0 }, fused: { x: 576, y: 0 }, reranked: { x: 864, y: 0 } };
// The spot beside `lexical` is taken by `fused`.
const BESIDE_TAKEN: EditorLayout = { question: { x: 0, y: 0 }, lexical: { x: 288, y: 0 }, vectors: { x: 288, y: 448 }, fused: { x: 576, y: 0 }, reranked: { x: 864, y: 0 } };

function Harness({ layout, start = null, initial = HYBRID }: { layout: EditorLayout; start?: string | null; initial?: WireDocument }) {
  const [client] = useState(() => createApiClient());
  const [selected, setSelected] = useState<string | null>(start);
  return (
    <div style={{ width: 1400, height: 800 }}>
      <Editor client={client} title="Draft" initial={initial} layout={layout} capabilities={WORKSPACE.capabilities} services={SERVICES.services} grammar={GRAMMAR} selected={selected} onSelect={setSelected} />
    </div>
  );
}

const setup = (props: Parameters<typeof Harness>[0]) => {
  mockApi({ 'POST /pipelines/validate': { body: { hash: 'a'.repeat(64), rendering: null } } });
  return render(<Harness {...props} />);
};
const posOf = (root: HTMLElement, id: string) => {
  const style = (root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement).style.transform;
  const [, x, y] = /translate\(([-\d.]+)px,\s*([-\d.]+)px\)/.exec(style)!;
  return { x: Number(x), y: Number(y) };
};
const overlaps = (a: { x: number; y: number }, b: { x: number; y: number }) => a.x < b.x + NODE_WIDTH && b.x < a.x + NODE_WIDTH && a.y < b.y + NODE_HEIGHT && b.y < a.y + NODE_HEIGHT;
const place = (name: RegExp) => fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name }));

describe('a node placed from the palette', () => {
  it('lands beside the selected node, moved clear of a card already there', () => {
    const { container } = setup({ layout: BESIDE_TAKEN, start: 'lexical' });
    place(/^rrf/);
    const at = posOf(container, 'rrf');
    expect(at.x).toBe(BESIDE_TAKEN.fused!.x);
    for (const id of ['question', 'lexical', 'vectors', 'fused', 'reranked']) expect(overlaps(at, posOf(container, id)), id).toBe(false);
  });

  it('with nothing selected, lands right of the graph, clear of every card', () => {
    const { container } = setup({ layout: BESIDE_TAKEN });
    place(/^rrf/);
    const at = posOf(container, 'rrf');
    expect(at.x).toBeGreaterThan(BESIDE_TAKEN.reranked!.x);
    for (const id of ['question', 'lexical', 'vectors', 'fused', 'reranked']) expect(overlaps(at, posOf(container, id)), id).toBe(false);
  });
});

describe('Tidy layout', () => {
  it('lays every node out as the automatic layout does, in one undo step', () => {
    const { container } = setup({ layout: PILED });
    fireEvent.click(screen.getByRole('button', { name: 'Tidy layout' }));
    const tidy = resolveLayout(toModel(toGraph(HYBRID, GRAMMAR))).positions;
    for (const id of Object.keys(PILED)) expect(posOf(container, id), id).toEqual(tidy[id]);
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(posOf(container, 'vectors')).toEqual(PILED.vectors);
  });
});
