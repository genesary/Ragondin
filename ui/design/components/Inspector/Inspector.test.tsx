/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Inspector.css?raw';
import { Inspector } from './Inspector.tsx';

describe('Inspector docked', () => {
  it('is a named panel with the family tile, the node name and its implementation in mono', () => {
    const { container } = render(
      <Inspector family="reranker" title="rerank" impl="reranker/onnx" onClose={() => {}}>
        body
      </Inspector>,
    );
    const panel = screen.getByRole('complementary', { name: 'rerank' });
    expect(panel.hasAttribute('data-floating')).toBe(false);
    expect(container.querySelector('.rg-tile[data-family="reranker"]')).toBeTruthy();
    expect(screen.getByText('reranker/onnx').classList.contains('rg-inspector__impl')).toBe(true);
    expect(screen.getByText('body').classList.contains('rg-inspector__body')).toBe(true);
    expect(declared(css, '.rg-inspector', 'width')).toBe('var(--size-inspector)');
    expect(declared(css, '.rg-inspector', 'border-left')).toBe('1px solid var(--line)');
  });

  it('closes from a named button', () => {
    const onClose = vi.fn();
    render(
      <Inspector family="retriever" title="bm25" onClose={onClose}>
        x
      </Inspector>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Close inspector' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('holds its footer actions at the bottom', () => {
    render(
      <Inspector family="fusion" title="rrf" footer={<button type="button">Run up to here</button>}>
        x
      </Inspector>,
    );
    expect(screen.getByRole('button', { name: 'Run up to here' }).closest('.rg-inspector__foot')).toBeTruthy();
  });
});

describe('Inspector floating', () => {
  it('floats on the layer, with the float shadow and the large radius', () => {
    render(
      <Inspector family="generator" title="answer" floating>
        x
      </Inspector>,
    );
    expect(screen.getByRole('complementary').hasAttribute('data-floating')).toBe(true);
    expect(declared(css, '.rg-inspector[data-floating]', 'background')).toBe('var(--layer)');
    expect(declared(css, '.rg-inspector[data-floating]', 'box-shadow')).toBe('var(--shadow-float)');
    expect(declared(css, '.rg-inspector[data-floating]', 'border-radius')).toBe('var(--radius-l)');
    expect(css).not.toMatch(/\.is-[a-z]/);
  });
});
