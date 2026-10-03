/** @vitest-environment happy-dom */
import { render, screen, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { describe, expect, it } from 'vitest';
import { declared, parseRules } from '../../design/testing/css.ts';
import css from './NodeCard.css?raw';
import { NodeCard, type NodeCardProps } from './NodeCard.tsx';
import portCss from './Port.css?raw';

const BASE: NodeCardProps = {
  family: 'reranker',
  name: 'reranked',
  impl: 'reranker/cross_encoder',
  param: { name: 'top_k', value: '10' },
  inputs: ['query', 'chunks'],
  output: 'chunks',
};

const THEMES = ['light', 'dark'] as const;

/** Renders the card under each theme and hands back each card element. */
function inThemes(ui: ReactElement): HTMLElement[] {
  const { container } = render(
    <>
      {THEMES.map((theme) => (
        <div key={theme} data-theme={theme}>
          {ui}
        </div>
      ))}
    </>,
  );
  return THEMES.map((theme) => container.querySelector(`[data-theme="${theme}"] .rg-node`) as HTMLElement);
}

describe('NodeCard, at rest', () => {
  it('shows the family tile with its glyph, the name, the implementation and the key parameter, in both themes', () => {
    for (const card of inThemes(<NodeCard {...BASE} />)) {
      expect(card.getAttribute('data-family')).toBe('reranker');
      const tile = card.querySelector('.rg-tile[data-family="reranker"]');
      expect(tile?.querySelector('svg')).toBeTruthy();
      expect(within(card).getByText('reranked')).toBeTruthy();
      expect(within(card).getByText('reranker/cross_encoder')).toBeTruthy();
      expect(within(card).getByText('top_k')).toBeTruthy();
      expect(within(card).getByText('10')).toBeTruthy();
    }
  });

  it('shows no parameter row when the node has no key parameter', () => {
    const { container } = render(<NodeCard {...BASE} param={undefined} />);
    expect(container.querySelector('.rg-node__param')).toBeNull();
  });

  it('draws one port per input, in order, and one output, each carrying its kind', () => {
    const { container } = render(<NodeCard {...BASE} />);
    const ins = [...container.querySelectorAll('.rg-port[data-side="in"]')];
    expect(ins.map((p) => p.getAttribute('data-kind'))).toEqual(['query', 'chunks']);
    expect(container.querySelectorAll('.rg-port[data-side="out"]')).toHaveLength(1);
    expect(container.querySelector('.rg-port[data-side="out"]')?.getAttribute('data-kind')).toBe('chunks');
  });

  it('draws no output port when the node has none', () => {
    const { container } = render(<NodeCard {...BASE} output={null} />);
    expect(container.querySelector('.rg-port[data-side="out"]')).toBeNull();
  });

  it('lets the canvas draw its own ports in their place', () => {
    const { container } = render(<NodeCard {...BASE} renderPort={(p) => <b key={`${p.side}${p.index}`} data-own={p.kind} />} />);
    expect([...container.querySelectorAll('[data-own]')].map((p) => p.getAttribute('data-own'))).toEqual(['query', 'chunks', 'chunks']);
    expect(container.querySelector('.rg-port')).toBeNull();
  });

  it('takes its border from line-strong, and line-control on hover, and is the one resting object with a shadow', () => {
    expect(declared(css, '.rg-node', 'border')).toBe('1px solid var(--line-strong)');
    expect(declared(css, '.rg-node', 'box-shadow')).toBe('var(--shadow-contact)');
    expect(declared(css, '.rg-node[data-preview-state="hover"]', 'border-color')).toBe('var(--line-control)');
    expect(declared(css, '.rg-node:hover', 'border-color')).toBe('var(--line-control)');
  });

  it('writes no colour of its own: every colour is a token', () => {
    for (const sheet of [css, portCss]) {
      expect(sheet).not.toMatch(/#[0-9a-f]{3,8}\b/i);
      expect(sheet).not.toMatch(/\b(rgb|hsl|oklch)a?\(/);
      expect(sheet).not.toMatch(/\.is-[a-z]/);
    }
  });
});

describe('NodeCard states', () => {
  it('selected: an accent border and halo, and the word in its data', () => {
    for (const card of inThemes(<NodeCard {...BASE} selected />)) expect(card.getAttribute('data-selected')).toBe('true');
    expect(declared(css, '.rg-node[data-selected="true"]', 'border-color')).toBe('var(--accent)');
    expect(declared(css, '.rg-node[data-selected="true"]', 'box-shadow')).toContain('var(--accent-wash)');
  });

  it('keyboard focus: the focus ring at 4px, on the focused canvas node or in the preview', () => {
    const rule = parseRules(css).find((r) => r.selector.includes('.react-flow__node:focus-visible > .rg-node'));
    expect(rule?.declarations.get('outline')).toBe('2px solid var(--focus-ring)');
    expect(rule?.declarations.get('outline-offset')).toBe('4px');
    expect(rule?.selector).toContain('.rg-node[data-preview-state="focus"]');
  });

  it('invalid: the critical border, the word with the alert glyph, and the message inside the card', () => {
    for (const card of inThemes(<NodeCard {...BASE} status={{ kind: 'invalid', message: 'top_k must be at least 1.' }} />)) {
      expect(card.getAttribute('data-status')).toBe('invalid');
      expect(within(card).getByText('invalid')).toBeTruthy();
      expect(within(card).getByText('top_k must be at least 1.')).toBeTruthy();
      expect(card.querySelector('.rg-node__msg svg')).toBeTruthy();
    }
    const rule = parseRules(css).find((r) => r.selector.includes('.rg-node[data-status="invalid"]'));
    expect(rule?.declarations.get('border-color')).toBe('var(--critical)');
  });

  it('running: the word and real, counted progress', () => {
    for (const card of inThemes(<NodeCard {...BASE} status={{ kind: 'running', value: 3982, total: 10570, label: '3,982 / 10,570' }} />)) {
      expect(card.getAttribute('data-status')).toBe('running');
      expect(within(card).getByText('running')).toBeTruthy();
      expect(within(card).getByText('3,982 / 10,570')).toBeTruthy();
    }
  });

  it('queued: the word beside the clock glyph', () => {
    for (const card of inThemes(<NodeCard {...BASE} status={{ kind: 'queued' }} />)) {
      expect(card.getAttribute('data-status')).toBe('queued');
      expect(within(card).getByText('queued')).toBeTruthy();
      expect(card.querySelector('.rg-node__state svg')).toBeTruthy();
    }
  });

  it('dragging: the lifted shadow', () => {
    for (const card of inThemes(<NodeCard {...BASE} dragging />)) expect(card.getAttribute('data-dragging')).toBe('true');
    expect(declared(css, '.rg-node[data-dragging="true"]', 'box-shadow')).toBe('var(--shadow-lift)');
  });

  it('ghost drop target: a dashed accent outline on the accent wash, and inert', () => {
    for (const card of inThemes(<NodeCard {...BASE} name="Drop to add" variant="ghost" />)) {
      expect(card.getAttribute('data-variant')).toBe('ghost');
      expect(card.getAttribute('aria-hidden')).toBe('true');
    }
    expect(declared(css, '.rg-node[data-variant="ghost"]', 'border')).toBe('1.5px dashed var(--accent)');
    expect(declared(css, '.rg-node[data-variant="ghost"]', 'background')).toBe('var(--accent-wash)');
  });

  it('not in this build: a dashed outline and faded content', () => {
    for (const card of inThemes(<NodeCard {...BASE} impl="not in this build" variant="unavailable" />)) {
      expect(card.getAttribute('data-variant')).toBe('unavailable');
      expect(within(card).getByText('not in this build')).toBeTruthy();
    }
    expect(declared(css, '.rg-node[data-variant="unavailable"]', 'border-style')).toBe('dashed');
  });

  it('control flow: the neutral diamond tile', () => {
    const { container } = render(<NodeCard {...BASE} family="control" name="branch" impl="extension/branch" />);
    expect(container.querySelector('.rg-tile[data-family="control"]')).toBeTruthy();
  });
});

describe('NodeCard with an overlay (replay)', () => {
  it('draws none of the overlay rows without an overlay', () => {
    const { container } = render(<NodeCard {...BASE} />);
    expect(container.querySelector('.rg-node__replay')).toBeNull();
    expect(container.querySelector('.rg-rankstrip')).toBeNull();
    expect(container.querySelector('.rg-node__dur')).toBeNull();
    expect(container.querySelector('.rg-node')?.getAttribute('data-replay')).toBeNull();
  });

  it('turns the ports into read-only dots and drops the parameter row', () => {
    const { container } = render(<NodeCard {...BASE} overlay={{}} />);
    expect(container.querySelector('.rg-node')?.getAttribute('data-replay')).toBe('true');
    expect(container.querySelector('.rg-node__param')).toBeNull();
  });

  it('a duration bar proportional to the node’s share, and the time beside it', () => {
    const { container } = render(<NodeCard {...BASE} overlay={{ durationMs: 349, share: 0.85 }} />);
    const bar = container.querySelector('.rg-node__dur b') as HTMLElement;
    expect(bar.style.width).toBe('85%');
    expect(screen.getByText('349 ms')).toBeTruthy();
    expect(container.querySelector('.rg-node__dur')?.getAttribute('title')).toBe("349 ms, 85% of this query's time");
  });

  it('the share alone draws the bar, and the time alone draws the time', () => {
    const share = render(<NodeCard {...BASE} overlay={{ share: 0.5 }} />);
    expect((share.container.querySelector('.rg-node__dur b') as HTMLElement).style.width).toBe('50%');
    expect(share.container.textContent).not.toContain(' ms');
    share.unmount();
    const time = render(<NodeCard {...BASE} overlay={{ durationMs: 12 }} />);
    expect(time.container.querySelector('.rg-node__dur b')).toBeNull();
    expect(screen.getByText('12 ms')).toBeTruthy();
  });

  it('a share too small to see still draws a sliver', () => {
    const { container } = render(<NodeCard {...BASE} overlay={{ share: 0.001 }} />);
    expect((container.querySelector('.rg-node__dur b') as HTMLElement).style.width).toBe('2%');
  });

  it('the metric text', () => {
    const { container } = render(<NodeCard {...BASE} overlay={{ metric: { name: 'nDCG@10', value: '0.861' } }} />);
    expect(within(container).getByText('nDCG@10')).toBeTruthy();
    expect(within(container).getByText('0.861')).toBeTruthy();
    expect(container.querySelector('.rg-rankstrip')).toBeNull();
  });

  it('a ten-cell rank strip filled at the given ranks, with its sentence', () => {
    const { container } = render(<NodeCard {...BASE} overlay={{ ranks: [1, 2] }} />);
    const cells = [...container.querySelectorAll('.rg-rankstrip > i')];
    expect(cells).toHaveLength(10);
    expect(cells.map((c, i) => (c.getAttribute('data-cell') === 'hit' ? i + 1 : 0)).filter(Boolean)).toEqual([1, 2]);
    expect(screen.getByRole('img', { name: '2 gold passages in the top 10, at rank 1, 2' })).toBeTruthy();
  });

  it('the discarded count', () => {
    render(<NodeCard {...BASE} overlay={{ discarded: 90 }} />);
    expect(screen.getByText('90 discarded')).toBeTruthy();
  });

  it('the failed state with its message', () => {
    for (const card of inThemes(<NodeCard {...BASE} overlay={{ error: 'the service did not answer' }} />)) {
      expect(card.getAttribute('data-status')).toBe('failed');
      expect(within(card).getByText('failed')).toBeTruthy();
      expect(within(card).getByText('the service did not answer')).toBeTruthy();
    }
  });

  it('the dashed absent state, with its tag in words', () => {
    for (const card of inThemes(<NodeCard {...BASE} overlay={{ onlyHere: 'only in B' }} />)) {
      expect(card.getAttribute('data-only-here')).toBe('true');
      expect(within(card).getByText('only in B')).toBeTruthy();
    }
    expect(declared(css, '.rg-node[data-only-here="true"]', 'border')).toBe('1.5px dashed var(--accent)');
  });

  it('the not-run state: the word, muted, and none of the result rows', () => {
    for (const card of inThemes(<NodeCard {...BASE} overlay={{ notRun: true }} />)) {
      expect(card.getAttribute('data-status')).toBe('not-run');
      expect(within(card).getByText('not run')).toBeTruthy();
      expect(card.querySelector('.rg-node__replay')).toBeNull();
    }
    expect(declared(css, '.rg-node[data-status="not-run"]', 'border-style')).toBe('dotted');
  });
});
