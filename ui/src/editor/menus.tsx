import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Glyph, type GlyphName } from '../../design/index.ts';
import { PORT_LABEL } from '../canvas/Port.tsx';
import type { WireDocument } from './document.ts';
import type { PaletteSection } from './Palette.tsx';
import { portsOf, type PortGrammar } from './ports.ts';

type ItemProps = { glyph?: GlyphName; title: string; line?: string | null; refused?: boolean; onChoose: () => void };

/**
 * One menu entry: a `menuitem` with a glyph, its name and an optional second
 * line. A refused entry stays focusable and says why on that line, and
 * choosing it does nothing. Enter and Space choose, as a click does.
 */
export function MenuItem({ glyph, title, line = null, refused = false, onChoose }: ItemProps) {
  const choose = () => {
    if (!refused) onChoose();
  };
  return (
    <button
      type="button"
      role="menuitem"
      aria-disabled={refused || undefined}
      onClick={(event) => {
        // Safari does not focus a clicked button; an entry that changes the
        // menu around itself must hold focus, or the menu would lose it.
        event.currentTarget.focus();
        choose();
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          choose();
        }
      }}
    >
      {glyph === undefined ? <span /> : <Glyph name={glyph} />}
      <span className="rg-menu__text">
        <b>{title}</b>
        {line === null ? null : <small>{line}</small>}
      </span>
    </button>
  );
}

export type NodeEntriesProps = {
  doc: WireDocument;
  grammar: PortGrammar | null;
  node: string;
  /** A declared input: it has nothing to open, duplicate, run or delete, and is connected like any node. */
  input?: boolean;
  refuse: (from: string, to: string, port: number) => string | null;
  close: () => void;
  onOpen: () => void;
  onDuplicate: () => void;
  onConnect: (to: string, port: number) => void;
  onDelete: () => void;
};

/**
 * The node menu's entries in write mode: "Open parameters", "Duplicate",
 * "Connect output to…" — the drag's keyboard equivalent, which turns the menu
 * into every input port of every other node, each open or refused with the
 * reason the drag would show — "Run up to this node", refused until #357
 * enables it, and "Delete node".
 */
export function NodeEntries({ doc, grammar, node, input = false, refuse, close, onOpen, onDuplicate, onConnect, onDelete }: NodeEntriesProps) {
  const [connecting, setConnecting] = useState(false);
  const act = (action: () => void) => () => {
    close();
    action();
  };
  // The entry that turns the menu into the port list, and back, keeps its key
  // and so its element: focus stays on it while the entries around it change.
  const toggle = (
    <MenuItem key="connect" glyph="link" title={connecting ? 'Back to the node menu' : 'Connect output to…'} onChoose={() => setConnecting(!connecting)} />
  );
  if (connecting) {
    const targets = doc.pipeline.nodes
      .filter((n) => n.id !== node)
      .flatMap((n) => portsOf(n, grammar).inputs.map((kind, port) => ({ to: n.id, port, kind })));
    return [
      toggle,
      ...targets.map(({ to, port, kind }) => {
        const reason = refuse(node, to, port);
        return (
          <MenuItem
            key={`${to}:${port}`}
            glyph="link"
            title={`${to}, port ${port}${kind === 'opaque' ? '' : ` (${PORT_LABEL[kind]})`}`}
            line={reason}
            refused={reason !== null}
            onChoose={act(() => onConnect(to, port))}
          />
        );
      }),
    ];
  }
  if (input) return [toggle];
  return [
    <MenuItem key="open" glyph="split" title="Open parameters" onChoose={act(onOpen)} />,
    <MenuItem key="duplicate" glyph="copy" title="Duplicate" onChoose={act(onDuplicate)} />,
    toggle,
    <MenuItem key="run" glyph="prefix" title="Run up to this node" line="Arrives with #357." refused onChoose={() => {}} />,
    <MenuItem key="delete" glyph="close" title="Delete node" onChoose={act(onDelete)} />,
  ];
}

export type InsertMenuProps = {
  sections: readonly PaletteSection[];
  onPlace: (component: string, impl: string) => void;
  /** Closes the list; focus goes back where it was. */
  onClose: () => void;
};

/**
 * The insert list `/` opens: the palette's entries as a menu, in the same
 * order, refused ones saying why. Focus moves to the first entry; the arrow
 * keys, Home and End move; Escape or Tab closes; focus leaving closes it.
 */
export function InsertMenu({ sections, onPlace, onClose }: InsertMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const closed = useRef(false);
  const close = () => {
    if (closed.current) return;
    closed.current = true;
    onClose();
  };
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, []);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const all = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = all.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      all[(to + all.length) % all.length]?.focus();
    };
    if (event.key === 'ArrowDown') move(at + 1);
    else if (event.key === 'ArrowUp') move(at - 1);
    else if (event.key === 'Home') move(0);
    else if (event.key === 'End') move(all.length - 1);
    else if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  };
  const items: ReactNode[] = sections.flatMap((section) =>
    section.entries.map((entry) => (
      <MenuItem
        key={`${section.label}/${entry.remote ? 'remote' : 'local'}/${entry.impl}`}
        title={entry.impl}
        line={entry.refused ?? `${section.label}${entry.remote ? ', Remote' : ''}`}
        refused={entry.refused !== null}
        onChoose={() => {
          closed.current = true;
          onPlace(entry.component, entry.impl);
        }}
      />
    )),
  );
  return (
    <div
      ref={ref}
      className="rg-menu rg-editor__insert"
      role="menu"
      aria-label="Insert a node"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) close();
      }}
    >
      {items}
    </div>
  );
}
