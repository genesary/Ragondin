/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Button.css?raw';
import { Button, type ButtonKind } from './Button.tsx';

const KINDS: ButtonKind[] = ['primary', 'secondary', 'quiet', 'destructive'];

describe('Button at rest', () => {
  it.each(KINDS)('%s draws its role, and says what happens in its label', (kind) => {
    render(<Button kind={kind}>Launch run</Button>);
    const button = screen.getByRole('button', { name: 'Launch run' });
    expect(button.classList.contains(`rg-btn--${kind}`)).toBe(true);
    expect(button.getAttribute('type')).toBe('button');
  });

  it('is secondary unless told otherwise', () => {
    render(<Button>Export YAML</Button>);
    expect(screen.getByRole('button').classList.contains('rg-btn--secondary')).toBe(true);
  });

  it.each([
    ['primary', 'var(--accent)', 'var(--on-accent)'],
    ['secondary', 'var(--surface)', 'var(--ink)'],
    ['quiet', 'transparent', 'var(--ink-2)'],
    ['destructive', 'var(--surface)', 'var(--critical)'],
  ])('%s takes its fill and ink from tokens', (kind, bg, fg) => {
    const own = (p: string) => declared(css, `.rg-btn--${kind}`, p) ?? declared(css, '.rg-btn', p);
    expect(own('--btn-bg')).toBe(bg);
    expect(own('--btn-fg')).toBe(fg);
  });

  it('carries a leading glyph when given one', () => {
    const { container } = render(<Button icon="play">Run the starter pipeline</Button>);
    expect(container.querySelector('button svg')).toBeTruthy();
  });
});

describe('Button hover', () => {
  it.each([
    ['.rg-btn--secondary:hover', 'var(--surface-2)'],
    ['.rg-btn--primary:hover', 'var(--accent-hover)'],
    ['.rg-btn--quiet:hover', 'var(--surface-2)'],
    ['.rg-btn--destructive:hover', 'var(--critical-wash)'],
  ])('%s changes its fill at once', (selector, bg) => {
    expect(declared(css, selector, '--btn-bg')).toBe(bg);
    expect(declared(css, selector.replace(':hover', '.is-hover'), '--btn-bg')).toBe(bg);
  });
});

describe('Button focus', () => {
  it('takes keyboard focus and draws the focus ring outside itself', () => {
    render(<Button kind="primary">Launch run</Button>);
    const button = screen.getByRole('button');
    button.focus();
    expect(document.activeElement).toBe(button);
    expect(declared(css, '.rg-btn.is-focus', 'outline')).toBe('2px solid var(--focus-ring)');
    expect(declared(css, '.rg-btn.is-focus', 'outline-offset')).toBe('2px');
  });
});

describe('Button pressed', () => {
  it('acts when pressed, and darkens one step', () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Download</Button>);
    fireEvent.click(screen.getByRole('button'));
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(declared(css, '.rg-btn--secondary:active', '--btn-bg')).toBe('var(--surface-3)');
    expect(declared(css, '.rg-btn--primary:active', '--btn-bg')).toBe('var(--accent-hover)');
  });
});

describe('Button disabled', () => {
  it('refuses, keeps its label legible and says why', () => {
    const onClick = vi.fn();
    render(
      <Button kind="primary" disabled disabledReason="Select runs on one benchmark to compare" onClick={onClick}>
        Compare 3 runs
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Compare 3 runs' });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(button.getAttribute('title')).toBe('Select runs on one benchmark to compare');
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(declared(css, '.rg-btn:disabled', '--btn-fg')).toBe('var(--ink-disabled)');
  });
});

describe('Button loading', () => {
  it('says the verb in progress, marks itself busy, and does not act twice', () => {
    const onClick = vi.fn();
    render(
      <Button kind="primary" busy busyLabel="Launching" onClick={onClick}>
        Launch run
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Launching' });
    expect(button.getAttribute('aria-busy')).toBe('true');
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(css).toMatch(/\.rg-btn\[aria-busy="true"\]::after/);
  });
});
