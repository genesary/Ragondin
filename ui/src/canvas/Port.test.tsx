/** @vitest-environment happy-dom */
// The port shapes, read from the styles the browser computes, with the
// tokens loaded: each kind must stay told apart by its mark in every state a
// port is drawn in — unconnected, connected, and replay — so colour is never
// what separates two kinds.
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import '../../design/base.css';
import { Canvas } from './Canvas.tsx';
import { GATED_GEN } from './fixtures.ts';
import type { PortKind } from './model.ts';
import { NodeCard, type NodeCardProps } from './NodeCard.tsx';

const KINDS: PortKind[] = ['query', 'chunks', 'context', 'answer', 'opaque'];

const STATES: Record<string, Partial<NodeCardProps>> = {
  unconnected: {},
  connected: { connected: { inputs: true, output: true } },
  replay: { connected: { inputs: true, output: true }, overlay: {} },
};

type Mark = { radius: string; transform: string; border: string; fill: string; dot: string };

function marks(state: Partial<NodeCardProps>): Record<string, Mark> {
  const { container } = render(<NodeCard family="control" name="all" impl="extension/all" inputs={KINDS} output={null} {...state} />);
  const surface = getComputedStyle(container.querySelector('.rg-node')!).backgroundColor;
  const out: Record<string, Mark> = {};
  for (const port of container.querySelectorAll<HTMLElement>('.rg-port')) {
    const cs = getComputedStyle(port);
    const dot = port.querySelector<HTMLElement>('.rg-port__dot');
    out[port.getAttribute('data-kind')!] = {
      radius: cs.borderRadius,
      transform: cs.transform,
      border: Number.parseFloat(cs.borderTopWidth) > 0 ? cs.borderTopStyle : 'none',
      fill: cs.backgroundColor === surface ? 'hollow' : 'filled',
      dot: dot === null || getComputedStyle(dot).display === 'none' ? 'none' : 'dot',
    };
  }
  return out;
}

describe('port marks', () => {
  for (const [name, state] of Object.entries(STATES)) {
    it(`tells all five kinds apart by shape when ${name}`, () => {
      const drawn = marks(state);
      expect(Object.keys(drawn).sort()).toEqual([...KINDS].sort());
      const signatures = KINDS.map((k) => JSON.stringify(drawn[k]));
      expect(new Set(signatures).size, signatures.join('\n')).toBe(KINDS.length);
    });

    it(`draws opaque as a hollow ring with a dot when ${name}`, () => {
      const opaque = marks(state)['opaque']!;
      expect(opaque.fill).toBe('hollow');
      expect(opaque.border).not.toBe('none');
      expect(opaque.dot).toBe('dot');
    });
  }

  it('fills a connected port and leaves an unconnected one hollow', () => {
    expect(marks(STATES['unconnected']!)['query']!.fill).toBe('hollow');
    expect(marks(STATES['connected']!)['query']!.fill).toBe('filled');
  });
});

describe('port marks on the real canvas, with its stylesheet', () => {
  it.each([
    ['at rest', undefined],
    ['in replay', { answer: {}, gate: {} }],
  ])('keeps a connected opaque port hollow, apart from a connected query port, with its dot showing, %s', (_, overlay) => {
    const { container } = render(<Canvas graph={GATED_GEN} label="gated" overlay={overlay} />);
    const port = (node: string, side: string, kind: string) =>
      container.querySelector<HTMLElement>(`.react-flow__node[data-id="${node}"] .rg-port[data-side="${side}"][data-kind="${kind}"]`)!;
    const opaqueIn = port('answer', 'in', 'opaque');
    const opaqueOut = port('gate', 'out', 'opaque');
    const queryIn = port('answer', 'in', 'query');
    for (const p of [opaqueIn, opaqueOut, queryIn]) expect(p.getAttribute('data-connected')).toBe('true');
    const fill = (el: HTMLElement) => getComputedStyle(el).backgroundColor;
    expect(fill(queryIn)).not.toBe('');
    for (const opaque of [opaqueIn, opaqueOut]) {
      expect(fill(opaque)).not.toBe(fill(queryIn));
      const dot = opaque.querySelector<HTMLElement>('.rg-port__dot')!;
      expect(fill(dot)).not.toBe(fill(opaque));
    }
  });
});
