/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './InlineMessage.css?raw';
import { InlineMessage } from './InlineMessage.tsx';

describe('InlineMessage critical', () => {
  it('interrupts, names the problem in bold beside a glyph that says "Error", and offers the fix', () => {
    render(
      <InlineMessage tone="critical" title="This run failed at node rerank on 300 of 300 queries." action={<button type="button">Open in Replay</button>}>
        The trace up to fusion is kept.
      </InlineMessage>,
    );
    const message = screen.getByRole('alert');
    expect(screen.getByRole('img', { name: 'Error' })).toBeTruthy();
    expect(message.querySelector('b')?.textContent).toBe('This run failed at node rerank on 300 of 300 queries.');
    expect(message.textContent).toContain('The trace up to fusion is kept.');
    expect(screen.getByRole('button', { name: 'Open in Replay' })).toBeTruthy();
    expect(declared(css, '.rg-inline', '--msg-bg')).toBe('var(--critical-wash)');
  });
});

describe('InlineMessage warning', () => {
  it('reads as a status, with a glyph that says "Warning"', () => {
    render(<InlineMessage tone="warning" title="beir/fiqa is downloading and not verified yet." />);
    expect(screen.getByRole('status')).toBeTruthy();
    expect(screen.getByRole('img', { name: 'Warning' })).toBeTruthy();
    expect(declared(css, '.rg-inline[data-tone="warning"]', '--msg-bg')).toBe('var(--warning-wash)');
  });

  it('is no live region of its own inside one its caller keeps (live={false}), and keeps its spoken tone', () => {
    const { container } = render(<InlineMessage tone="warning" live={false} title="1 fault beside this job." />);
    expect(screen.queryByRole('status')).toBeNull();
    expect(container.querySelector('.rg-inline')?.hasAttribute('role')).toBe(false);
    expect(screen.getByRole('img', { name: 'Warning' })).toBeTruthy();
  });
});

describe('InlineMessage info', () => {
  it('is a quiet note, with a glyph that says "Note"', () => {
    const { container } = render(<InlineMessage tone="info" title="Adding a judge changes the run's hash." />);
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('img', { name: 'Note' })).toBeTruthy();
    expect(container.querySelector('.rg-inline')?.getAttribute('data-tone')).toBe('info');
    expect(declared(css, '.rg-inline[data-tone="info"]', '--msg-bg')).toBe('var(--surface-2)');
  });
});
