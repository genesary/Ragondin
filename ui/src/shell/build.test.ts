/** @vitest-environment happy-dom */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RELOAD_KEY, judgeBuild } from './build.ts';

beforeEach(() => {
  window.sessionStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('judgeBuild', () => {
  it('lets the page continue when the server is this build, and forgets any earlier reload', () => {
    window.sessionStorage.setItem(RELOAD_KEY, '1.0.0+aaaaaaaaaaaa');
    expect(judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+bbbbbbbbbbbb')).toEqual({ verdict: 'same' });
    expect(window.sessionStorage.getItem(RELOAD_KEY)).toBeNull();
  });

  it('asks for one reload when another build answers, remembering which', () => {
    expect(judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+aaaaaaaaaaaa')).toEqual({ verdict: 'reload' });
    expect(window.sessionStorage.getItem(RELOAD_KEY)).toBe('1.0.0+bbbbbbbbbbbb');
  });

  it('refuses, naming both builds, when the same other build answers after that reload', () => {
    judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+aaaaaaaaaaaa');
    expect(judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+aaaaaaaaaaaa')).toEqual({
      verdict: 'different',
      served: '1.0.0+bbbbbbbbbbbb',
      expected: '1.0.0+aaaaaaaaaaaa',
    });
  });

  it('reloads again for a third build, since that one was never tried', () => {
    judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+aaaaaaaaaaaa');
    expect(judgeBuild('1.0.0+cccccccccccc', '1.0.0+aaaaaaaaaaaa')).toEqual({ verdict: 'reload' });
  });

  it('treats an answer without an identity as another build', () => {
    expect(judgeBuild(null, '1.0.0+aaaaaaaaaaaa')).toEqual({ verdict: 'reload' });
    expect(judgeBuild(null, '1.0.0+aaaaaaaaaaaa')).toEqual({ verdict: 'different', served: null, expected: '1.0.0+aaaaaaaaaaaa' });
  });

  it('refuses at once rather than reload when it cannot remember having reloaded, which could loop', () => {
    vi.spyOn(window, 'sessionStorage', 'get').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    expect(judgeBuild('1.0.0+bbbbbbbbbbbb', '1.0.0+aaaaaaaaaaaa')).toEqual({
      verdict: 'different',
      served: '1.0.0+bbbbbbbbbbbb',
      expected: '1.0.0+aaaaaaaaaaaa',
    });
  });
});
