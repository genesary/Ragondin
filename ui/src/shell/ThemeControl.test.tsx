/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { THEME_KEY, ThemeControl } from './ThemeControl.tsx';

const root = () => document.documentElement;
const choose = (label: string) => fireEvent.click(screen.getByRole('radio', { name: label }));
const checked = () => screen.getAllByRole('radio').find((r) => r.getAttribute('aria-checked') === 'true')?.textContent;

beforeEach(() => {
  window.localStorage.clear();
  root().removeAttribute('data-theme');
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ThemeControl', () => {
  it('follows the system by default: no data-theme on the page', () => {
    render(<ThemeControl />);
    expect(screen.getByRole('radiogroup', { name: 'Theme' })).toBeTruthy();
    expect(checked()).toBe('System');
    expect(root().hasAttribute('data-theme')).toBe(false);
  });

  it('switches to light, to dark and back to the system', () => {
    render(<ThemeControl />);
    choose('Light');
    expect(root().getAttribute('data-theme')).toBe('light');
    choose('Dark');
    expect(root().getAttribute('data-theme')).toBe('dark');
    choose('System');
    expect(root().hasAttribute('data-theme')).toBe(false);
  });

  it('remembers the choice for this viewer, so it survives a reload', () => {
    const { unmount } = render(<ThemeControl />);
    choose('Dark');
    expect(window.localStorage.getItem(THEME_KEY)).toBe('dark');
    unmount();
    root().removeAttribute('data-theme');

    render(<ThemeControl />);
    expect(checked()).toBe('Dark');
    expect(root().getAttribute('data-theme')).toBe('dark');
  });

  it('ignores a remembered value it does not know', () => {
    window.localStorage.setItem(THEME_KEY, 'sepia');
    render(<ThemeControl />);
    expect(checked()).toBe('System');
  });

  it('works without storage: the choice applies to the page and is simply not remembered', () => {
    window.localStorage.setItem(THEME_KEY, 'dark');
    vi.spyOn(window, 'localStorage', 'get').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    render(<ThemeControl />);
    expect(checked()).toBe('System');
    choose('Light');
    expect(checked()).toBe('Light');
    expect(root().getAttribute('data-theme')).toBe('light');
  });
});
