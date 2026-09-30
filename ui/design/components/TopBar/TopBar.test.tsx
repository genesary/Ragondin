/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
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
    expect(declared(css, '.rg-topbar', 'height')).toBe('var(--size-topbar)');
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
    expect(declared(css, '.rg-dot', 'background')).toBe('var(--good)');
  });

  it('shows an unreachable service as a hollow ring and the word "unreachable": shape and word, not colour', () => {
    const { container } = render(<TopBar workspace="w" links={links} services={[{ name: 'bge-embedder', connected: false }]} />);
    const service = container.querySelector('.rg-service') as HTMLElement;
    expect(service.textContent).toBe('bge-embedder unreachable');
    expect(service.getAttribute('data-connected')).toBe('false');
    expect(declared(css, '.rg-service[data-connected="false"] .rg-dot', 'background')).toBe('transparent');
    expect(css).not.toMatch(/\.is-[a-z]/);
  });

  it('keeps a slot at the end for the theme switch', () => {
    render(<TopBar workspace="w" links={links} services={[]} end={<button type="button">Theme</button>} />);
    expect(screen.getByRole('button', { name: 'Theme' }).closest('.rg-topbar__end')).toBeTruthy();
  });
});
