/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './FilterChip.css?raw';
import { FilterChip } from './FilterChip.tsx';

describe('FilterChip at rest', () => {
  it('is a toggle, unpressed, with its count', () => {
    const onToggle = vi.fn();
    const { container } = render(<FilterChip label="beir/scifact" count={12} pressed={false} onToggle={onToggle} />);
    const chip = screen.getByRole('button', { name: /beir\/scifact/ });
    expect(chip.getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByText('12').classList.contains('rg-count')).toBe(true);
    expect(container.querySelector('svg')).toBeNull();
    fireEvent.click(chip);
    expect(onToggle).toHaveBeenCalledWith(true);
  });
});

describe('FilterChip hover', () => {
  it('washes its fill', () => {
    expect(declared(css, '.rg-filter:hover', 'background')).toBe('var(--surface-2)');
    expect(css).not.toMatch(/\.is-[a-z]/);
  });
});

describe('FilterChip pressed', () => {
  it('fills with the accent and gains a check: shape and fill both change', () => {
    const onToggle = vi.fn();
    const { container } = render(<FilterChip label="done" pressed onToggle={onToggle} />);
    const chip = screen.getByRole('button', { name: /done/ });
    expect(chip.getAttribute('aria-pressed')).toBe('true');
    expect(container.querySelector('svg')).toBeTruthy();
    fireEvent.click(chip);
    expect(onToggle).toHaveBeenCalledWith(false);
    expect(declared(css, '.rg-filter[aria-pressed="true"]', 'background')).toBe('var(--accent)');
  });
});

describe('FilterChip disabled', () => {
  it('stays visible and says why, the reason describing the chip rather than naming it', () => {
    render(<FilterChip label="beir/fiqa" pressed={false} onToggle={() => {}} disabled disabledReason="not downloaded" />);
    const chip = screen.getByRole('button', { name: 'beir/fiqa' }) as HTMLButtonElement;
    expect(chip.disabled).toBe(true);
    expect(chip.textContent).not.toContain('not downloaded');
    expect(document.getElementById(chip.getAttribute('aria-describedby') ?? '')?.textContent).toBe('not downloaded');
  });
});
