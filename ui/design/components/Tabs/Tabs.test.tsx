/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Tabs.css?raw';
import { Tabs } from './Tabs.tsx';

const tabs = [
  { id: 'workspace', label: 'Workspace' },
  { id: 'benchmarks', label: 'Benchmarks', count: 3 },
  { id: 'services', label: 'Services' },
];

describe('Tabs at rest', () => {
  it('is a tab list with the selected tab marked and in the tab order', () => {
    render(<Tabs label="Setup sections" tabs={tabs} selected="benchmarks" onSelect={() => {}} />);
    expect(screen.getByRole('tablist', { name: 'Setup sections' })).toBeTruthy();
    const selected = screen.getByRole('tab', { name: /Benchmarks/ });
    expect(selected.getAttribute('aria-selected')).toBe('true');
    expect(selected.tabIndex).toBe(0);
    expect(screen.getByRole('tab', { name: 'Workspace' }).tabIndex).toBe(-1);
  });

  it('carries a count in a neutral pill, never in colour', () => {
    render(<Tabs label="s" tabs={tabs} selected="workspace" onSelect={() => {}} />);
    expect(screen.getByText('3').classList.contains('rg-count')).toBe(true);
    expect(declared(css, '.rg-tabs .rg-count', 'background')).toBe('var(--surface-3)');
  });
});

describe('Tabs selected', () => {
  it('sets the selected label in ink with an accent underline: a mark, not only a colour', () => {
    expect(declared(css, '.rg-tabs button[aria-selected="true"]', 'color')).toBe('var(--ink)');
    expect(declared(css, '.rg-tabs button[aria-selected="true"]::after', 'background')).toBe('var(--accent)');
    expect(declared(css, '.rg-tabs button[aria-selected="true"]::after', 'height')).toBe('2px');
  });

  it('selects on click', () => {
    const onSelect = vi.fn();
    render(<Tabs label="s" tabs={tabs} selected="workspace" onSelect={onSelect} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Services' }));
    expect(onSelect).toHaveBeenCalledWith('services');
  });
});

describe('Tabs hover', () => {
  it('darkens the label', () => {
    expect(declared(css, '.rg-tabs button:hover', 'color')).toBe('var(--ink)');
  });
});

describe('Tabs keyboard', () => {
  it('moves along the list with the arrow keys, wrapping', () => {
    const onSelect = vi.fn();
    render(<Tabs label="s" tabs={tabs} selected="workspace" onSelect={onSelect} />);
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Workspace' }), { key: 'ArrowLeft' });
    expect(onSelect).toHaveBeenLastCalledWith('services');
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Workspace' }), { key: 'ArrowRight' });
    expect(onSelect).toHaveBeenLastCalledWith('benchmarks');
  });
});
