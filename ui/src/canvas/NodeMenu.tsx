import { useEffect, useRef, type FocusEvent, type KeyboardEvent, type ReactNode } from 'react';

export type NodeMenuProps = {
  node: string;
  /** Closes the menu; `refocus` gives focus back to the node. */
  onClose: (refocus: boolean) => void;
  /** The entries, each a `menuitem`; with none, the shell says there is nothing to do here. */
  children?: ReactNode;
};

/**
 * The node's menu shell: a floating layer opened on the focused node by
 * Shift+F10, the context-menu key or a right-click (the canvas handles
 * those). Focus moves to the first entry when it opens; the arrow keys, Home
 * and End move between entries; Escape or Tab closes it and gives focus back
 * to the node; focus leaving it closes it. It closes once: the focus a key's
 * close moves away is not a second close.
 */
export function NodeMenu({ node, onClose, children }: NodeMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const closed = useRef(false);
  const close = (refocus: boolean) => {
    if (closed.current) return;
    closed.current = true;
    onClose(refocus);
  };
  const items = () => [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
  useEffect(() => {
    (items()[0] ?? ref.current)?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const all = items();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      all[(to + all.length) % all.length]?.focus();
    };
    if (event.key === 'ArrowDown') move(at + 1);
    else if (event.key === 'ArrowUp') move(at - 1);
    else if (event.key === 'Home') move(0);
    else if (event.key === 'End') move(all.length - 1);
    else if (event.key === 'Tab' || event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  };
  const onBlur = (event: FocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) close(false);
  };

  return (
    // A click inside is the entry's to act on: it must not reach the node, whose click selects it and closes this menu.
    <div ref={ref} className="rg-menu nodrag nopan" role="menu" aria-label={`Node ${node}`} tabIndex={-1} onKeyDown={onKeyDown} onBlur={onBlur} onClick={(event) => event.stopPropagation()}>
      {children ?? (
        <button type="button" role="menuitem" aria-disabled="true">
          No actions here
        </button>
      )}
    </div>
  );
}
