/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './SegmentedControl.css?raw';
import { SegmentedControl } from './SegmentedControl.tsx';

const options = [
  { value: 'single', label: 'Single' },
  { value: 'side', label: 'Side by side' },
  { value: 'diff', label: 'Diff', disabled: true, reason: 'Pick two runs to diff' },
];

describe('SegmentedControl at rest', () => {
  it('is one group of mutually exclusive modes with the pressed one checked', () => {
    render(<SegmentedControl label="Replay mode" options={options} value="single" onChange={() => {}} />);
    expect(screen.getByRole('radiogroup', { name: 'Replay mode' })).toBeTruthy();
    expect(screen.getByRole('radio', { name: 'Single' }).getAttribute('aria-checked')).toBe('true');
    expect(screen.getByRole('radio', { name: 'Side by side' }).getAttribute('aria-checked')).toBe('false');
  });

  it('is one tab stop: only the pressed option is in the tab order', () => {
    render(<SegmentedControl label="m" options={options} value="side" onChange={() => {}} />);
    expect(screen.getAllByRole('radio').map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
  });
});

describe('SegmentedControl pressed', () => {
  it('raises the pressed thumb on the surface with the contact shadow', () => {
    expect(declared(css, '.rg-seg button[aria-checked="true"]', 'background')).toBe('var(--seg-thumb)');
    expect(declared(css, '.rg-seg button[aria-checked="true"]', 'box-shadow')).toBe('var(--shadow-contact)');
  });

  it('switches on click', () => {
    const onChange = vi.fn();
    render(<SegmentedControl label="m" options={options} value="single" onChange={onChange} />);
    fireEvent.click(screen.getByRole('radio', { name: 'Side by side' }));
    expect(onChange).toHaveBeenCalledWith('side');
  });
});

describe('SegmentedControl keyboard', () => {
  it('moves with the arrow keys, skipping a disabled option and wrapping', () => {
    const onChange = vi.fn();
    render(<SegmentedControl label="m" options={options} value="side" onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole('radio', { name: 'Side by side' }), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('single');
    fireEvent.keyDown(screen.getByRole('radio', { name: 'Side by side' }), { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('single');
  });
});

describe('SegmentedControl keyboard, continued', () => {
  it('keeps the arrow keys from scrolling the page', () => {
    render(<SegmentedControl label="m" options={options} value="side" onChange={() => {}} />);
    expect(fireEvent.keyDown(screen.getByRole('radio', { name: 'Side by side' }), { key: 'ArrowRight' })).toBe(false);
    expect(fireEvent.keyDown(screen.getByRole('radio', { name: 'Side by side' }), { key: 'a' })).toBe(true);
  });

  it('keeps a tab stop on the first option when the value matches none', () => {
    render(<SegmentedControl label="m" options={options} value="gone" onChange={() => {}} />);
    expect(screen.getAllByRole('radio').map((r) => r.tabIndex)).toEqual([0, -1, -1]);
  });
});

describe('SegmentedControl disabled option', () => {
  it('refuses and gives its reason', () => {
    const onChange = vi.fn();
    render(<SegmentedControl label="m" options={options} value="single" onChange={onChange} />);
    const diff = screen.getByRole('radio', { name: 'Diff' }) as HTMLButtonElement;
    expect(diff.disabled).toBe(true);
    expect(diff.getAttribute('title')).toBe('Pick two runs to diff');
  });
});
