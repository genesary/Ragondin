/** @vitest-environment happy-dom */
import { render, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { FAMILIES, FAMILY_LABEL } from '../glyphs/Glyph.tsx';
import { declared } from '../testing/css.ts';
import { COMPONENTS, Preview } from './Preview.tsx';
import css from './preview.css?raw';

const LISTED = [
  'Glyph', 'Button', 'Input', 'Select', 'Checkbox', 'StatusChip', 'MetricChip', 'FilterChip', 'RunSwatch', 'Table',
  'Sheet', 'Inspector', 'Toast', 'InlineMessage', 'Progress', 'EmptyState', 'RankStrip', 'SegmentedControl', 'Tabs', 'TopBar',
  'StatusDot', 'NodeCard', 'Canvas', 'Charts',
];

describe('Preview', () => {
  it('shows every primitive the design system commits', () => {
    expect([...COMPONENTS].sort()).toEqual([...LISTED].sort());
  });

  it('renders every component in both themes, side by side', () => {
    const { container } = render(<Preview />);
    const light = container.querySelector('[data-theme="light"]') as HTMLElement;
    const dark = container.querySelector('[data-theme="dark"]') as HTMLElement;
    expect(light).toBeTruthy();
    expect(dark).toBeTruthy();
    for (const name of LISTED) {
      expect(within(light).getByRole('heading', { name })).toBeTruthy();
      expect(within(dark).getByRole('heading', { name })).toBeTruthy();
    }
  });

  it('writes every family\'s name on its pigment, in that pigment\'s own ink, in both themes', () => {
    const { container } = render(<Preview />);
    for (const theme of ['light', 'dark']) {
      const column = container.querySelector(`[data-theme="${theme}"]`) as HTMLElement;
      const names = [...column.querySelectorAll<HTMLElement>('.rg-preview__on-family')];
      expect(names.map((n) => [n.dataset.family, n.textContent])).toEqual(FAMILIES.map((f) => [f, FAMILY_LABEL[f]]));
    }
    for (const family of FAMILIES.filter((f) => f !== 'control')) {
      expect(declared(css, `.rg-preview__on-family[data-family="${family}"]`, 'background')).toBe(`var(--family-${family})`);
      expect(declared(css, `.rg-preview__on-family[data-family="${family}"]`, 'color')).toBe(`var(--on-family-${family})`);
    }
    expect(declared(css, '.rg-preview__on-family', 'background')).toBe('var(--family-query)');
    expect(declared(css, '.rg-preview__on-family', 'color')).toBe('var(--on-family-query)');
  });

  it('forces hover, focus and pressed at rest through data-preview-state, and through no class', () => {
    const { container } = render(<Preview />);
    for (const state of ['hover', 'focus', 'pressed']) expect(container.querySelectorAll(`[data-preview-state="${state}"]`).length).toBeGreaterThan(0);
    expect(container.querySelector('[class*="is-"]')).toBeNull();
  });
});
