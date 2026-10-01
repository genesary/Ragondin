import { useEffect, useRef, type ReactNode } from 'react';

export type NodeMenuProps = {
  node: string;
  /** The entries, each a `menuitem`; with none, the shell says there is nothing to do here. */
  children?: ReactNode;
};

/**
 * The node's menu shell: a floating layer opened on the focused node by
 * Shift+F10, the context-menu key or a right-click, and closed by Escape (the
 * canvas handles both keys). Focus moves to the first entry when it opens.
 */
export function NodeMenu({ node, children }: NodeMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const first = ref.current?.querySelector<HTMLElement>('[role="menuitem"]');
    (first ?? ref.current)?.focus();
  }, []);
  return (
    <div ref={ref} className="rg-menu nodrag nopan" role="menu" aria-label={`Node ${node}`} tabIndex={-1}>
      {children ?? (
        <button type="button" role="menuitem" aria-disabled="true">
          No actions here
        </button>
      )}
    </div>
  );
}
