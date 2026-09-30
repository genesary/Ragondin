/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Input.css?raw';
import { Input } from './Input.tsx';

describe('Input at rest', () => {
  it('names the field by its parameter and keeps a stable id', () => {
    render(<Input id="top-k" label="top_k" defaultValue="100" />);
    const input = screen.getByLabelText('top_k');
    expect(input.id).toBe('top-k');
    expect((input as HTMLInputElement).value).toBe('100');
    expect(input.getAttribute('aria-invalid')).toBeNull();
  });

  it('describes itself with its helper text', () => {
    render(<Input id="k" label="top_k" help="1 to 5,183" />);
    const input = screen.getByLabelText('top_k');
    const help = document.getElementById(input.getAttribute('aria-describedby') ?? '');
    expect(help?.textContent).toBe('1 to 5,183');
  });

  it('carries its unit as a suffix that assistive technology reads too, right-aligned numbers and a mono face on request', () => {
    const { container } = render(<Input id="t" label="timeout" unit="s" numeric mono help="1 to 600" />);
    const unit = screen.getByText('s');
    expect(unit.classList.contains('rg-affix__unit')).toBe(true);
    expect(unit.getAttribute('aria-hidden')).toBeNull();
    const described = (screen.getByLabelText('timeout').getAttribute('aria-describedby') ?? '').split(' ');
    expect(described).toContain(unit.id);
    expect(described.map((i) => document.getElementById(i)?.textContent)).toEqual(['s', '1 to 600']);
    const input = container.querySelector('input');
    expect(input?.classList.contains('rg-input--num')).toBe(true);
    expect(input?.classList.contains('rg-input--mono')).toBe(true);
    expect(declared(css, '.rg-input--num', 'text-align')).toBe('right');
  });
});

describe('Input hover', () => {
  it('darkens its boundary', () => {
    expect(declared(css, '.rg-input:hover', 'border-color')).toBe('var(--ink-3)');
    expect(declared(css, '.rg-input[data-preview-state="hover"]', 'border-color')).toBe('var(--ink-3)');
    expect(css).not.toMatch(/\.is-(hover|focus)\b/);
  });
});

describe('Input focus', () => {
  it('takes focus and draws the ring at 1px offset', () => {
    render(<Input id="f" label="path" />);
    const input = screen.getByLabelText('path');
    input.focus();
    expect(document.activeElement).toBe(input);
    expect(declared(css, '.rg-input:focus-visible', 'outline')).toBe('2px solid var(--focus-ring)');
    expect(declared(css, '.rg-input:focus-visible', 'outline-offset')).toBe('1px');
  });
});

describe('Input invalid', () => {
  it('says what is wrong in words beside a glyph, not in colour alone', () => {
    const { container } = render(<Input id="k" label="top_k" defaultValue="5000" error="5000 is more than the corpus holds. Use 1 to 5,183." />);
    const input = screen.getByLabelText('top_k');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    const message = document.getElementById(input.getAttribute('aria-describedby') ?? '');
    expect(message?.textContent).toBe('5000 is more than the corpus holds. Use 1 to 5,183.');
    expect(message?.querySelector('svg')).toBeTruthy();
    expect(container.querySelector('.rg-help[data-error]')).toBe(message);
    expect(declared(css, '.rg-input[aria-invalid="true"]', 'border-color')).toBe('var(--critical)');
  });
});

describe('Input disabled', () => {
  it('refuses input', () => {
    render(<Input id="d" label="embedder" disabled />);
    expect((screen.getByLabelText('embedder') as HTMLInputElement).disabled).toBe(true);
    expect(declared(css, '.rg-input:disabled', 'color')).toBe('var(--ink-disabled)');
  });
});

describe('Input read-only', () => {
  it('stays selectable and copyable, not disabled', () => {
    render(<Input id="r" label="embedder" readOnly defaultValue="bge-small" />);
    const input = screen.getByLabelText('embedder') as HTMLInputElement;
    expect(input.readOnly).toBe(true);
    expect(input.disabled).toBe(false);
  });
});
