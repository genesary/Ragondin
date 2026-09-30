/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Button.css?raw';
import { Button, ButtonLink, type ButtonKind } from './Button.tsx';

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

  it('ships no presentational state class: the preview forces a state with a data attribute', () => {
    expect(css).not.toMatch(/\.is-(hover|focus|pressed)\b/);
  });
});

describe('Button hover', () => {
  it.each([
    ['secondary', 'var(--surface-2)'],
    ['primary', 'var(--accent-hover)'],
    ['quiet', 'var(--surface-2)'],
    ['destructive', 'var(--critical-wash)'],
  ])('%s changes its fill at once', (kind, bg) => {
    expect(declared(css, `.rg-btn--${kind}:hover`, '--btn-bg')).toBe(bg);
    expect(declared(css, `.rg-btn--${kind}[data-preview-state="hover"]`, '--btn-bg')).toBe(bg);
  });
});

describe.each(KINDS)('Button %s focus', (kind) => {
  it('takes keyboard focus and draws the focus ring outside itself', () => {
    render(<Button kind={kind}>Launch run</Button>);
    const button = screen.getByRole('button');
    button.focus();
    expect(document.activeElement).toBe(button);
    for (const selector of ['.rg-btn:focus-visible', '.rg-btn[data-preview-state="focus"]']) {
      expect(declared(css, selector, 'outline')).toBe('2px solid var(--focus-ring)');
      expect(declared(css, selector, 'outline-offset')).toBe('2px');
    }
  });
});

describe.each([
  ['secondary', 'var(--surface-3)'],
  ['primary', 'var(--accent-hover)'],
  ['quiet', 'var(--surface-3)'],
  ['destructive', 'var(--critical-wash)'],
] as [ButtonKind, string][])('Button %s pressed', (kind, bg) => {
  it('acts when pressed, and darkens one step', () => {
    const onClick = vi.fn();
    render(
      <Button kind={kind} onClick={onClick}>
        Download
      </Button>,
    );
    fireEvent.click(screen.getByRole('button'));
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(declared(css, `.rg-btn--${kind}:active`, '--btn-bg')).toBe(bg);
    expect(declared(css, `.rg-btn--${kind}[data-preview-state="pressed"]`, '--btn-bg')).toBe(bg);
  });
});

describe.each(KINDS)('Button %s disabled', (kind) => {
  it('refuses, stays focusable so its reason can be reached without a pointer, and says why', () => {
    const onClick = vi.fn();
    render(
      <Button kind={kind} disabled disabledReason="Select runs on one benchmark to compare" onClick={onClick}>
        Compare 3 runs
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Compare 3 runs' }) as HTMLButtonElement;
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.disabled).toBe(false);
    button.focus();
    expect(document.activeElement).toBe(button);
    const reason = document.getElementById(button.getAttribute('aria-describedby') ?? '');
    expect(reason?.textContent).toBe('Select runs on one benchmark to compare');
    // One accessible path for the reason: the description, not a title as well.
    expect(button.getAttribute('title')).toBeNull();
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(declared(css, '.rg-btn[aria-disabled="true"]', '--btn-fg')).toBe('var(--ink-disabled)');
  });
});

describe.each(KINDS)('Button %s loading', (kind) => {
  it('says the verb in progress, marks itself busy, and does not act twice', () => {
    const onClick = vi.fn();
    render(
      <Button kind={kind} busy busyLabel="Launching" onClick={onClick}>
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

describe('Button disabled inside a form', () => {
  it('does not submit, by click or by Enter or Space, even when the caller asks for a submit button', () => {
    const onSubmit = vi.fn((e: { preventDefault: () => void }) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <Button type="submit" kind="primary" disabled disabledReason="Fix 2 fields first">
          Launch run
        </Button>
      </form>,
    );
    const button = screen.getByRole('button', { name: 'Launch run' }) as HTMLButtonElement;
    expect(button.type).toBe('button');
    fireEvent.click(button);
    for (const key of ['Enter', ' ']) {
      fireEvent.keyDown(button, { key });
      fireEvent.keyUp(button, { key });
    }
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('submits once enabled again, keeping the type the caller asked for', () => {
    const onSubmit = vi.fn((e: { preventDefault: () => void }) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <Button type="submit" kind="primary">
          Launch run
        </Button>
      </form>,
    );
    const button = screen.getByRole('button', { name: 'Launch run' }) as HTMLButtonElement;
    expect(button.type).toBe('submit');
    fireEvent.click(button);
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it('does not submit a second time while busy', () => {
    const onSubmit = vi.fn((e: { preventDefault: () => void }) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <Button type="submit" kind="primary" busy busyLabel="Launching">
          Launch run
        </Button>
      </form>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Launching' }));
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('runs none of the caller’s key, pointer or mouse press handlers while disabled', () => {
    const onKeyDown = vi.fn();
    const onPointerDown = vi.fn();
    const onMouseUp = vi.fn();
    render(
      <Button disabled disabledReason="Not yet" onKeyDown={onKeyDown} onPointerDown={onPointerDown} onMouseUp={onMouseUp}>
        Launch run
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Launch run' });
    fireEvent.keyDown(button, { key: 'Enter' });
    fireEvent.pointerDown(button);
    fireEvent.mouseUp(button);
    expect(onKeyDown).not.toHaveBeenCalled();
    expect(onPointerDown).not.toHaveBeenCalled();
    expect(onMouseUp).not.toHaveBeenCalled();
  });

  it('runs no activation handler of the caller’s while disabled', () => {
    const handlers = {
      onDoubleClick: vi.fn(),
      onKeyUp: vi.fn(),
      onPointerUp: vi.fn(),
      onMouseDown: vi.fn(),
      onTouchStart: vi.fn(),
      onTouchEnd: vi.fn(),
      onSubmit: vi.fn(),
      onKeyPress: vi.fn(),
      onAuxClick: vi.fn(),
      onContextMenu: vi.fn(),
    };
    render(
      <Button disabled disabledReason="Not yet" {...handlers}>
        Launch run
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Launch run' });
    fireEvent.doubleClick(button);
    fireEvent.keyUp(button, { key: 'Enter' });
    fireEvent.pointerUp(button);
    fireEvent.mouseDown(button);
    fireEvent.touchStart(button);
    fireEvent.touchEnd(button);
    fireEvent.submit(button);
    fireEvent.keyPress(button, { key: 'Enter', charCode: 13 });
    fireEvent(button, new MouseEvent('auxclick', { bubbles: true, button: 1 }));
    fireEvent.contextMenu(button);
    for (const handler of Object.values(handlers)) expect(handler).not.toHaveBeenCalled();
  });

  // A tooltip saying why a button refuses is wired through focus and hover,
  // and is most useful exactly while the button is disabled.
  it('passes the caller’s focus and hover handlers through while disabled', () => {
    const handlers = {
      onFocus: vi.fn(),
      onBlur: vi.fn(),
      onMouseEnter: vi.fn(),
      onMouseLeave: vi.fn(),
      onPointerEnter: vi.fn(),
      onPointerLeave: vi.fn(),
    };
    render(
      <Button disabled disabledReason="Not yet" {...handlers}>
        Launch run
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Launch run' });
    fireEvent.focus(button);
    fireEvent.blur(button);
    fireEvent.mouseEnter(button);
    fireEvent.mouseLeave(button);
    fireEvent.pointerEnter(button);
    fireEvent.pointerLeave(button);
    for (const handler of Object.values(handlers)) expect(handler).toHaveBeenCalledTimes(1);
  });

  it('keeps the caller’s own description beside the reason', () => {
    render(
      <>
        <p id="hint">Runs take about a minute.</p>
        <Button disabled disabledReason="Not yet" aria-describedby="hint">
          Launch run
        </Button>
      </>,
    );
    const ids = (screen.getByRole('button').getAttribute('aria-describedby') ?? '').split(' ');
    expect(ids.map((id) => document.getElementById(id)?.textContent)).toEqual(['Runs take about a minute.', 'Not yet']);
  });
});

describe('ButtonLink', () => {
  // A move to another view is a link, so middle-click, a new tab and "copy
  // link" work; it looks like the button of its kind and size.
  it('is a real link drawn as a button of its kind and size', () => {
    render(
      <ButtonLink kind="primary" size="l" href="#runs">
        Open Runs
      </ButtonLink>,
    );
    const link = screen.getByRole('link', { name: 'Open Runs' });
    expect(link.getAttribute('href')).toBe('#runs');
    expect([...link.classList]).toEqual(['rg-btn', 'rg-btn--primary', 'rg-btn--l']);
    expect(declared(css, 'a.rg-btn', 'text-decoration')).toBe('none');
  });

  it('is secondary and medium unless told otherwise', () => {
    render(<ButtonLink href="#setup">Open Setup</ButtonLink>);
    expect([...screen.getByRole('link').classList]).toEqual(['rg-btn', 'rg-btn--secondary']);
  });
});
