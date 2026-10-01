/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Checkbox.css?raw';
import { Checkbox } from './Checkbox.tsx';

describe('Checkbox unchecked', () => {
  it('is a labelled native checkbox whose whole row is the target', () => {
    const onChange = vi.fn();
    const { container } = render(<Checkbox label="hybrid-rerank" checked={false} onChange={onChange} />);
    const box = screen.getByRole('checkbox', { name: 'hybrid-rerank' }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(screen.getByText('hybrid-rerank'));
    expect(onChange).toHaveBeenCalledWith(true);
    expect(container.querySelector('label.rg-check')).toBeTruthy();
    expect(declared(css, '.rg-check', 'min-height')).toBe('var(--size-control)');
  });
});

describe('Checkbox checked', () => {
  it('is checked, and shows a check mark: shape, not only fill', () => {
    render(<Checkbox label="dense-only" checked onChange={() => {}} />);
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
    expect(declared(css, '.rg-check input:checked::before', 'transform')).toBe('scale(1)');
    expect(declared(css, '.rg-check input:checked', 'background')).toBe('var(--accent)');
  });
});

describe('Checkbox mixed', () => {
  it('is indeterminate when some of a group is selected, drawn as a bar', () => {
    render(<Checkbox label="All runs on beir/scifact" checked={false} indeterminate onChange={() => {}} />);
    expect((screen.getByRole('checkbox') as HTMLInputElement).indeterminate).toBe(true);
    expect(declared(css, '.rg-check input:indeterminate::before', 'clip-path')).toBe('inset(40% 10% 40% 10%)');
  });
});

describe('Checkbox disabled', () => {
  it('stays visible and says why, in words', () => {
    render(<Checkbox label="squad/dev" checked={false} onChange={() => {}} disabled disabledReason="another benchmark, can't join this comparison" />);
    const box = screen.getByRole('checkbox', { name: 'squad/dev' }) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    const reason = screen.getByText("another benchmark, can't join this comparison");
    expect(box.getAttribute('aria-describedby')).toBe(reason.id);
  });
});

describe('Checkbox named apart from its label', () => {
  it('takes an accessible name that says which item it selects, keeping its visible label', () => {
    render(<Checkbox label="beir/scifact" accessibleLabel="Select run a1b2c3d4e5f6 on beir/scifact" checked={false} onChange={() => {}} />);
    expect(screen.getByRole('checkbox', { name: 'Select run a1b2c3d4e5f6 on beir/scifact' })).toBeTruthy();
    expect(screen.getByText('beir/scifact')).toBeTruthy();
  });
});

describe('Checkbox inside a one-tab-stop group', () => {
  it('can leave the tab order, while a click still toggles it', () => {
    const onChange = vi.fn();
    render(<Checkbox label="beir/scifact" checked={false} onChange={onChange} tabIndex={-1} />);
    const box = screen.getByRole('checkbox');
    expect(box.getAttribute('tabindex')).toBe('-1');
    fireEvent.click(box);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
