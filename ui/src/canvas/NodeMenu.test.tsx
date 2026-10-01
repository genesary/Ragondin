/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { NodeMenu } from './NodeMenu.tsx';

// The canvas gives focus back to the node when the menu closes; this stands
// in for it, so the focus leaving the menu is the one a real close causes.
function setup() {
  const outside = document.createElement('button');
  document.body.append(outside);
  const onClose = vi.fn(() => outside.focus());
  render(
    <NodeMenu node="fused" onClose={onClose}>
      <button role="menuitem">one</button>
      <button role="menuitem">two</button>
    </NodeMenu>,
  );
  return { onClose, outside };
}

describe('NodeMenu closing', () => {
  it.each(['Escape', 'Tab'])('closes once on %s, asking for focus back on the node', (key) => {
    const { onClose } = setup();
    fireEvent.keyDown(screen.getAllByRole('menuitem')[0]!, { key });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledWith(true);
  });

  it('closes once when focus leaves it, leaving focus where it went', () => {
    const { onClose, outside } = setup();
    onClose.mockImplementation(() => {});
    act(() => outside.focus());
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledWith(false);
  });

  it('does not close while focus moves between its entries', () => {
    const { onClose } = setup();
    fireEvent.keyDown(screen.getAllByRole('menuitem')[0]!, { key: 'ArrowDown' });
    expect(onClose).not.toHaveBeenCalled();
  });
});
