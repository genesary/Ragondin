/** @vitest-environment happy-dom */
import { render, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { COMPONENTS, Preview } from './Preview.tsx';

const LISTED = [
  'Glyph', 'Button', 'Input', 'Select', 'Checkbox', 'StatusChip', 'MetricChip', 'FilterChip', 'RunSwatch', 'Table',
  'Sheet', 'Inspector', 'Toast', 'InlineMessage', 'Progress', 'EmptyState', 'RankStrip', 'SegmentedControl', 'Tabs', 'TopBar',
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
});
