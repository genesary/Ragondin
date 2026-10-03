/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared, parseRules } from '../../testing/css.ts';
import dotCss from '../StatusDot/StatusDot.css?raw';
import css from './TopBar.css?raw';
import { TopBar } from './TopBar.tsx';

const links = [
  { label: 'Runs', href: '#runs' },
  { label: 'Compare', href: '#compare', current: true },
];

describe('TopBar shell', () => {
  it('sets the name in type, the workspace path in mono, and the screens as real links', () => {
    render(<TopBar workspace="~/ragondin-ws" links={links} services={[]} />);
    expect(screen.getByRole('banner')).toBeTruthy();
    expect(screen.getByText('Ragondin').classList.contains('rg-wordmark')).toBe(true);
    expect(screen.getByText('~/ragondin-ws').classList.contains('rg-crumb')).toBe(true);
    expect(screen.getByRole('link', { name: 'Runs' }).getAttribute('href')).toBe('#runs');
  });
});

describe('TopBar at a narrow width', () => {
  // happy-dom lays nothing out, so these assert the rules that keep the bar
  // inside the viewport; the widths themselves are measured in a browser.
  it('wraps its parts onto further lines rather than widening the page, one bar high at least', () => {
    expect(declared(css, '.rg-topbar', 'flex-wrap')).toBe('wrap');
    expect(declared(css, '.rg-topbar', 'min-height')).toBe('var(--size-topbar)');
    expect(declared(css, '.rg-topbar', 'height')).toBeUndefined();
  });

  it('lets the workspace break inside itself when its line is narrower than it', () => {
    expect(declared(css, '.rg-crumb', 'min-width')).toBe('0');
    expect(declared(css, '.rg-crumb', 'overflow-wrap')).toBe('anywhere');
    expect(declared(css, '.rg-crumb', 'white-space')).toBeUndefined();
    expect(declared(css, '.rg-crumb', 'height')).toBeUndefined();
  });

  it('wraps the screens inside the nav, each line bar-high and spaced so the current mark lands where it does in one line', () => {
    expect(declared(css, '.rg-nav', 'flex-wrap')).toBe('wrap');
    expect(declared(css, '.rg-nav', 'min-height')).toBe('calc(var(--size-topbar) - 1px)');
    // The mark hangs 8px under its link; a row gap of 16px keeps it off the next row.
    expect(declared(css, '.rg-nav a[aria-current="page"]::after', 'bottom')).toBe('-8px');
    expect(declared(css, '.rg-nav', 'row-gap')).toBe('var(--space-4)');
  });

  it('gives every line of a wrapped bar room, at any width: the name and the workspace a control high, the end bar-high', () => {
    // Each is centred in the one line and no taller than it, so one line is unchanged.
    expect(declared(css, '.rg-wordmark', 'min-height')).toBe('var(--size-control)');
    expect(declared(css, '.rg-wordmark', 'align-items')).toBe('center');
    expect(declared(css, '.rg-crumb', 'min-height')).toBe('var(--size-control)');
    expect(declared(css, '.rg-topbar__end', 'min-height')).toBe('calc(var(--size-topbar) - 1px)');
    // Room comes from the lines themselves, not from padding a single line would also take.
    expect(declared(css, '.rg-topbar', 'padding')).toBe('0 var(--space-4)');
    expect(parseRules(css).filter((r) => r.atRule !== null && r.selector === '.rg-topbar')).toEqual([]);
  });

  it('tightens the screens at phone width so the six share one row, the mark kept inside each link', () => {
    const phone = parseRules(css).filter((r) => r.atRule === '@media (max-width: 640px)');
    const link = phone.find((r) => r.selector === '.rg-nav a');
    const mark = phone.find((r) => r.selector === '.rg-nav a[aria-current="page"]::after');
    expect(link?.declarations.get('padding')).toBe('0 var(--space-2)');
    expect(mark?.declarations.get('left')).toBe('var(--space-2)');
    expect(mark?.declarations.get('right')).toBe('var(--space-2)');
  });
});

describe('TopBar workspace', () => {
  it('holds whatever the application puts there, such as an indicator that is a link', () => {
    render(<TopBar workspace={<a href="#setup">~/ragondin-ws, 2 services</a>} links={links} services={[]} />);
    const indicator = screen.getByRole('link', { name: '~/ragondin-ws, 2 services' });
    expect(indicator.closest('.rg-crumb')).toBeTruthy();
    expect(indicator.closest('nav')).toBeNull();
  });
});

describe('TopBar current screen', () => {
  it('marks the current screen by aria-current, weight and an accent mark, not colour alone', () => {
    render(<TopBar workspace="w" links={links} services={[]} />);
    expect(screen.getByRole('link', { name: 'Compare' }).getAttribute('aria-current')).toBe('page');
    expect(screen.getByRole('link', { name: 'Runs' }).getAttribute('aria-current')).toBeNull();
    expect(declared(css, '.rg-nav a[aria-current="page"]', 'font-weight')).toBe('600');
    expect(declared(css, '.rg-nav a[aria-current="page"]::after', 'background')).toBe('var(--accent)');
  });
});

describe('TopBar services', () => {
  it('shows a connected service as a filled dot and its name', () => {
    const { container } = render(<TopBar workspace="w" links={links} services={[{ name: 'qwen2.5-7b', connected: true }]} />);
    const service = container.querySelector('.rg-service') as HTMLElement;
    expect(service.textContent).toBe('qwen2.5-7b');
    expect(service.getAttribute('data-connected')).toBe('true');
    expect(service.querySelector('.rg-dot')?.getAttribute('data-connected')).toBe('true');
    expect(declared(dotCss, '.rg-dot', 'background')).toBe('var(--good)');
  });

  it('shows an unreachable service as a hollow ring and the word "unreachable": shape and word, not colour', () => {
    const { container } = render(<TopBar workspace="w" links={links} services={[{ name: 'bge-embedder', connected: false }]} />);
    const service = container.querySelector('.rg-service') as HTMLElement;
    expect(service.textContent).toBe('bge-embedder unreachable');
    expect(service.getAttribute('data-connected')).toBe('false');
    expect(service.querySelector('.rg-dot')?.getAttribute('data-connected')).toBe('false');
    expect(declared(dotCss, '.rg-dot[data-connected="false"]', 'background')).toBe('transparent');
    expect(css).not.toMatch(/\.is-[a-z]/);
  });

  it('shows the application’s status — a stream, say — as a live pill with its dot and its word', () => {
    const { container, rerender } = render(<TopBar workspace="w" links={links} services={[]} status={{ label: 'connected', connected: true }} />);
    const pill = container.querySelector('[role="status"]') as HTMLElement;
    expect(pill.classList.contains('rg-service')).toBe(true);
    expect(pill.textContent).toBe('connected');
    expect(pill.querySelector('.rg-dot')?.getAttribute('data-connected')).toBe('true');
    rerender(<TopBar workspace="w" links={links} services={[]} status={{ label: 'disconnected — retrying', connected: false }} />);
    expect(pill.textContent).toBe('disconnected — retrying');
    expect(pill.querySelector('.rg-dot')?.getAttribute('data-connected')).toBe('false');
  });

  it('shows no status pill without a status', () => {
    const { container } = render(<TopBar workspace="w" links={links} services={[]} />);
    expect(container.querySelector('[role="status"]')).toBeNull();
  });

  it('keeps a slot at the end for the theme switch', () => {
    render(<TopBar workspace="w" links={links} services={[]} end={<button type="button">Theme</button>} />);
    expect(screen.getByRole('button', { name: 'Theme' }).closest('.rg-topbar__end')).toBeTruthy();
  });
});
