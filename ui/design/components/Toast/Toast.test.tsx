/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Toast.css?raw';
import { Toast } from './Toast.tsx';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('Toast confirmation', () => {
  it('says what just happened beside a check, politely, and offers Undo', () => {
    const onUndo = vi.fn();
    const { container } = render(<Toast action={{ label: 'Undo', onClick: onUndo }}>Deleted node rerank</Toast>);
    const toast = screen.getByRole('status');
    expect(toast.textContent).toContain('Deleted node rerank');
    expect(container.querySelector('.rg-toast > svg')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(onUndo).toHaveBeenCalledTimes(1);
    expect(declared(css, '.rg-toast', 'box-shadow')).toBe('var(--shadow-float)');
    expect(declared(css, '.rg-toast', 'background')).toBe('var(--layer)');
  });
});

describe('Toast critical', () => {
  it('interrupts, with an alert glyph and the words, and links to the cause', () => {
    const { container } = render(
      <Toast tone="critical" action={{ label: 'Open Setup', onClick: () => {} }}>
        Launch refused: generator qwen2.5-7b is unreachable.
      </Toast>,
    );
    expect(screen.getByRole('alert').textContent).toContain('Launch refused');
    expect(container.querySelector('.rg-toast')?.getAttribute('data-tone')).toBe('critical');
    expect(container.querySelector('.rg-toast > svg')).toBeTruthy();
  });
});

describe('Toast lifetime', () => {
  it('leaves after six seconds', () => {
    const onDismiss = vi.fn();
    render(<Toast onDismiss={onDismiss}>Run launched</Toast>);
    act(() => vi.advanceTimersByTime(5999));
    expect(onDismiss).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('waits while hovered or focused', () => {
    const onDismiss = vi.fn();
    render(<Toast onDismiss={onDismiss}>Run launched</Toast>);
    const toast = screen.getByRole('status');
    fireEvent.mouseEnter(toast);
    act(() => vi.advanceTimersByTime(10000));
    expect(onDismiss).not.toHaveBeenCalled();
    fireEvent.mouseLeave(toast);
    act(() => vi.advanceTimersByTime(6000));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('waits while something inside it has keyboard focus', () => {
    const onDismiss = vi.fn();
    render(<Toast onDismiss={onDismiss} action={{ label: 'Undo', onClick: () => {} }}>Deleted node rerank</Toast>);
    fireEvent.focus(screen.getByRole('button', { name: 'Undo' }));
    act(() => vi.advanceTimersByTime(10000));
    expect(onDismiss).not.toHaveBeenCalled();
    fireEvent.blur(screen.getByRole('button', { name: 'Undo' }));
    act(() => vi.advanceTimersByTime(6000));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('can be dismissed by hand', () => {
    const onDismiss = vi.fn();
    render(<Toast onDismiss={onDismiss}>Copied</Toast>);
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
