/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Words } from './words.tsx';

describe('Words', () => {
  it('draws each backticked span as code, never its backticks', () => {
    const { container } = render(<Words text="`a` feeding `b` would close a cycle" />);
    expect([...container.querySelectorAll('code')].map((c) => c.textContent)).toEqual(['a', 'b']);
    expect(container.textContent).toBe('a feeding b would close a cycle');
  });

  it('keeps an unmatched backtick as it is', () => {
    const { container } = render(<Words text="`a` and a lone ` mark" />);
    expect([...container.querySelectorAll('code')].map((c) => c.textContent)).toEqual(['a']);
    expect(container.textContent).toBe('a and a lone ` mark');
  });

  it('draws text with no backtick as text', () => {
    const { container } = render(<Words text="plain words" />);
    expect(container.querySelector('code')).toBeNull();
    expect(container.textContent).toBe('plain words');
  });
});
