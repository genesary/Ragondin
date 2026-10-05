/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Select.css?raw';
import { Select } from './Select.tsx';

const options = [
  { value: 'scifact', label: 'beir/scifact' },
  { value: 'fiqa', label: 'beir/fiqa' },
];

describe('Select at rest', () => {
  it('is the native element, labelled, with a drawn chevron', () => {
    const { container } = render(<Select id="bench" label="Benchmark" options={options} defaultValue="fiqa" />);
    const select = screen.getByLabelText('Benchmark') as HTMLSelectElement;
    expect(select.tagName).toBe('SELECT');
    expect(select.value).toBe('fiqa');
    expect(screen.getAllByRole('option')).toHaveLength(2);
    expect(container.querySelector('.rg-select svg')).toBeTruthy();
  });

  it('reports a change', () => {
    const onChange = vi.fn();
    render(<Select id="b" label="Benchmark" options={options} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText('Benchmark'), { target: { value: 'fiqa' } });
    expect(onChange).toHaveBeenCalled();
  });
});

describe('Select described by more than its help', () => {
  it('keeps its help line and adds the description the caller names', () => {
    render(
      <>
        <Select id="bench" label="Benchmark" options={options} help="Ready ones only." aria-describedby="note" />
        <p id="note">Not offered: squad/dev.</p>
      </>,
    );
    expect(screen.getByLabelText('Benchmark').getAttribute('aria-describedby')).toBe('bench-help note');
  });

  it('is described by the caller’s description alone when it has no help', () => {
    render(<Select id="bench" label="Benchmark" options={options} aria-describedby="note" />);
    expect(screen.getByLabelText('Benchmark').getAttribute('aria-describedby')).toBe('note');
  });
});

describe('Select focus', () => {
  it('takes focus and draws the ring', () => {
    render(<Select id="b" label="Benchmark" options={options} />);
    const select = screen.getByLabelText('Benchmark');
    select.focus();
    expect(document.activeElement).toBe(select);
    // :focus, not :focus-visible: a select opened with the mouse shows its ring too.
    expect(declared(css, '.rg-select select:focus', 'outline')).toBe('2px solid var(--focus-ring)');
    expect(css).not.toMatch(/\.is-(hover|focus)\b/);
  });
});

describe('Select disabled', () => {
  it('refuses a choice', () => {
    render(<Select id="b" label="Benchmark" options={options} disabled />);
    expect((screen.getByLabelText('Benchmark') as HTMLSelectElement).disabled).toBe(true);
    expect(declared(css, '.rg-select select:disabled', 'color')).toBe('var(--ink-disabled)');
  });
});

describe('Select invalid', () => {
  it('says what is wrong in words beside a glyph, the error drawn from its attribute', () => {
    expect(css).toMatch(/\.rg-select select\[aria-invalid="true"\]/);
    render(<Select id="b" label="Benchmark" options={options} error="beir/fiqa is not downloaded." />);
    const select = screen.getByLabelText('Benchmark');
    expect(select.getAttribute('aria-invalid')).toBe('true');
    const message = document.getElementById(select.getAttribute('aria-describedby') ?? '');
    expect(message?.textContent).toBe('beir/fiqa is not downloaded.');
    expect(message?.querySelector('svg')).toBeTruthy();
  });
});
