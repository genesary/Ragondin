/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Sheet.css?raw';
import { Section, Sheet } from './Sheet.tsx';

describe('Sheet', () => {
  it('is the page: a surface with one hairline border and no shadow', () => {
    const { container } = render(<Sheet>content</Sheet>);
    expect(container.querySelector('.rg-sheet')?.textContent).toBe('content');
    expect(declared(css, '.rg-sheet', 'background')).toBe('var(--surface)');
    expect(declared(css, '.rg-sheet', 'border')).toBe('1px solid var(--line)');
    expect(declared(css, '.rg-sheet', 'box-shadow')).toBe('none');
  });

  it('divides into flush sections by hairlines, each headed by what it shows and how to read it', () => {
    render(
      <Sheet>
        <Section heading="nDCG@10 at the output of each node" caption="read from the traces, not from a separate run">
          chart
        </Section>
        <Section heading="Latency per node">table</Section>
      </Sheet>,
    );
    expect(screen.getByRole('heading', { level: 2, name: 'nDCG@10 at the output of each node' })).toBeTruthy();
    expect(screen.getByText('read from the traces, not from a separate run').classList.contains('rg-section__caption')).toBe(true);
    expect(screen.getByRole('region', { name: 'Latency per node' })).toBeTruthy();
    expect(declared(css, '.rg-section + .rg-section', 'border-top')).toBe('1px solid var(--line)');
  });

  it('takes the heading level the page needs', () => {
    render(
      <Sheet>
        <Section heading="Services" level={3}>
          x
        </Section>
      </Sheet>,
    );
    expect(screen.getByRole('heading', { level: 3, name: 'Services' })).toBeTruthy();
  });

  it('can be the place an address moves to: given an anchor, it takes focus by script and never by Tab', () => {
    const anchor = createRef<HTMLElement>();
    render(
      <Sheet>
        <Section heading="Benchmarks">a</Section>
        <Section heading="Services" anchor={anchor}>
          b
        </Section>
      </Sheet>,
    );
    const services = screen.getByRole('region', { name: 'Services' });
    expect(anchor.current).toBe(services);
    expect(services.getAttribute('tabindex')).toBe('-1');
    anchor.current?.focus();
    expect(document.activeElement).toBe(services);
    expect(screen.getByRole('region', { name: 'Benchmarks' }).hasAttribute('tabindex')).toBe(false);
  });
});
