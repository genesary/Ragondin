/** @vitest-environment happy-dom */
// The port shapes, read from the styles the browser computes, with the
// tokens loaded: each kind must stay told apart by its mark in every state a
// port is drawn in — unconnected, connected, and replay — so colour is never
// what separates two kinds.
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import '../../design/base.css';
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
