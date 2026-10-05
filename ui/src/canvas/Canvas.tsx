import {
  Background,
  BackgroundVariant,
  Handle,
  Position as Side,
  ReactFlow,
  ReactFlowProvider,
  ViewportPortal,
  useReactFlow,
  useStore,
  useViewport,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeChange,
  type NodeHandle,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import { memo, useCallback, useEffect, useId, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';
import type { Graph } from '../api/types.ts';
import { Button, FAMILY_LABEL, Glyph } from '../../design/index.ts';
import { EdgeLine, edgePath } from './Edge.tsx';
import { GRID, NODE_WIDTH, boundsOf, fitArea, nodeSize, resolveLayout, type Box, type Position, type StoredLayout } from './layout.ts';
import { Legend } from './Legend.tsx';
import { describeOverlay, toModel, type CanvasNode, type NodeOverlay, type PortKind } from './model.ts';
import { NodeCard, type NodeStatus } from './NodeCard.tsx';
import { NodeMenu } from './NodeMenu.tsx';
import { PortDot, portTitle, portTop, type PortProps } from './Port.tsx';
import './Canvas.css';

/** What the canvas lets a person do: read (pan, zoom, select, the menu), or write — move nodes and draw edges too. */
export type CanvasMode = 'read' | 'write';

// What each mode allows. Read: pan, zoom, select, open the menu; nothing
// that moves a node or makes an edge. Write: nodes move, by drag and by the
// arrow keys, and an edge is drawn from an output port to an input port. The
// library's own connection mechanism stays off in both: write mode draws its
// edges itself, so every port can say during the drag why it refuses one.
const EDITABLE: Record<CanvasMode, boolean> = { read: false, write: true };

/** The ports a node draws in write mode: its input slots in port order, and its output. */
export type CanvasPorts = { inputs: readonly PortKind[]; output: PortKind | null };

/** The type a dragged palette entry carries, so the canvas takes a drop of nothing else. */
export const DROP_TYPE = 'application/x-ragondin-node';

export type CanvasProps = {
  /** The lowered graph, as `GET /runs/{id}` serves it. */
  graph: Graph;
  /** The canvas's accessible name, e.g. the pipeline's. */
  label: string;
  mode?: CanvasMode;
  /** Stored positions by node id. Absent, or missing a node, the automatic layout places it. */
  layout?: StoredLayout | undefined;
  /** Per node, what the caller knows of its execution for one query. It never moves a node. */
  overlay?: Readonly<Record<string, NodeOverlay>> | undefined;
  /**
   * In read mode, the extent the view is fitted to: at least this, from the
   * graph's top-left card, so canvases of one size given one frame — two
   * graphs side by side — share a zoom and an origin and line up. Absent,
   * the view is fitted to the graph alone.
   */
  frame?: { width: number; height: number } | undefined;
  /** The selected node. Pass it to control the selection; leave it out and the canvas keeps its own. */
  selected?: string | null;
  onSelect?: (id: string | null) => void;
  /** Called with the positions the automatic layout gave the nodes the stored layout did not name, so the caller can persist them. */
  onAutoPlaced?: (placed: Record<string, Position>) => void;
  /** What sits beside the canvas while a node is selected. */
  inspector?: (id: string) => ReactNode;
  /** The node menu's entries, and a way for an entry to close the menu once it has acted. */
  menu?: (id: string, close: () => void) => ReactNode;
  // Write mode. Each is read only there.
  /** Per node, the ports it draws, in place of those its edges imply — so an empty slot shows. */
  ports?: Readonly<Record<string, CanvasPorts>> | undefined;
  /** Why an edge from `from` into port `port` of `to` cannot be made, or null. Asked of every port while an edge is drawn. */
  refuse?: (from: string, to: string, port: number) => string | null;
  /** An edge was dropped on a port that took it. */
  onConnect?: (from: string, to: string, port: number) => void;
  /** A node was moved, by drag or by key, to this grid-snapped position. */
  onMove?: (id: string, position: Position) => void;
  /** `/` was pressed in the canvas: the caller opens its insert list. */
  onInsert?: () => void;
  /** A palette entry was dropped: its `DROP_TYPE` data, and where it landed, grid-snapped. */
  onDropItem?: (item: string, position: Position) => void;
  /** Per node, the validation's words, drawn as the invalid state. */
  issues?: Readonly<Record<string, string>> | undefined;
  /** The edges the validation named, by `edgeId`. */
  invalidEdges?: readonly string[] | undefined;
};

/** Where a port stands while an edge is drawn: open to it, or refusing it with a reason. */
type Drop = { state: 'open' } | { state: 'refused'; reason: string };

type WriteHandlers = {
  start: (node: string) => void;
  enter: (node: string, port: number) => void;
  leave: () => void;
  drop: (node: string, port: number) => void;
  more: (node: string) => void;
};

type CardData = {
  node: CanvasNode;
  overlay: NodeOverlay | undefined;
  selected: boolean;
  /** Which input ports an edge meets, in port order: drawn filled. */
  connectedInputs: readonly boolean[];
  /** Whether an edge leaves the node: its output port is drawn filled. */
  downstream: boolean;
  status: NodeStatus | undefined;
  /** While an edge is drawn, each input port's stance. */
  drops: readonly (Drop | null)[] | null;
  write: WriteHandlers | null;
  menu: ReactNode | null;
};
type CardNode = Node<CardData, 'card'>;
type LineEdge = Edge<{ port: number; kind: PortKind; invalid: boolean }, 'line'>;

// The description every node points at: the keys that work in its mode, and
// no other. The library's default names Space, the arrow keys and Delete,
// which the canvas does not bind as the library would.
const NODE_DESCRIPTION: Record<CanvasMode, string> = {
  read: 'Enter selects, Shift+F10 opens the menu, Escape clears.',
  write: 'Enter selects, Shift+F10 opens the menu, the arrow keys move it, Escape clears.',
};
// The id the library gives that description, suffixed with the flow's id.
const KEYS_DESCRIPTION = 'react-flow__node-desc';
const ariaLabels = (mode: CanvasMode) => ({
  'node.a11yDescription.default': NODE_DESCRIPTION[mode],
  'node.a11yDescription.keyboardDisabled': NODE_DESCRIPTION[mode],
  'edge.a11yDescription.default': '',
});
const ARIA_LABELS: Record<CanvasMode, ReturnType<typeof ariaLabels>> = { read: ariaLabels('read'), write: ariaLabels('write') };

// Where a port's edge attaches: the port's outer side, at its centre. The
// canvas library reads these before it has measured a card, so an edge is
// right from the first frame, and measures the drawn ports afterwards.
const PORT = 10;
const PORT_OVERHANG = 6;
const handles = (node: CanvasNode): NodeHandle[] => [
  ...node.inputs.map((_, i) => ({ id: `in-${i}`, type: 'target' as const, position: Side.Left, x: -PORT_OVERHANG, y: portTop(i), width: PORT, height: PORT })),
  ...(node.output === null ? [] : [{ id: 'out', type: 'source' as const, position: Side.Right, x: NODE_WIDTH + PORT_OVERHANG - PORT, y: portTop(0), width: PORT, height: PORT }]),
];

// A key's move, in grid steps: one, or four with Shift.
const ARROWS: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
const snap = (v: number) => Math.round(v / GRID) * GRID;
const WRITE_FIT = { maxZoom: 1 };
// The library's own fit leaves a tenth of the pane around the graph; a frame keeps the same margin.
const FRAME_FIT = { padding: 0.1 };

function accessibleName(node: CanvasNode, selected: boolean, overlay: NodeOverlay | undefined, issue: string | undefined): string {
  return [
    `${FAMILY_LABEL[node.family]} ${node.id}, ${node.impl}`,
    selected ? 'selected' : null,
    issue === undefined ? null : 'invalid',
    overlay?.error !== undefined ? 'failed' : null,
    overlay?.notRun === true ? 'not run' : null,
    overlay?.onlyHere ?? null,
  ]
    .filter((part) => part !== null)
    .join(', ');
}

/** A port drawn as a library handle, so an edge attaches to it; in write mode, an end an edge is drawn from or dropped on. */
const handlePort =
  (id: string, drops: readonly (Drop | null)[] | null, write: WriteHandlers | null) =>
  ({ side, kind, index, top, connected }: PortProps) => {
    const drop = side === 'in' ? (drops?.[index] ?? null) : null;
    const events =
      write === null
        ? {}
        : side === 'out'
          ? {
              onPointerDown: (event: PointerEvent) => {
                if (event.button !== 0) return;
                event.stopPropagation();
                write.start(id);
              },
            }
          : { onPointerEnter: () => write.enter(id, index), onPointerLeave: write.leave, onPointerUp: () => write.drop(id, index) };
    return (
      <Handle
        key={`${side}-${index}`}
        id={side === 'in' ? `in-${index}` : 'out'}
        type={side === 'in' ? 'target' : 'source'}
        position={side === 'in' ? Side.Left : Side.Right}
        isConnectable={false}
        isConnectableStart={false}
        isConnectableEnd={false}
        className={write === null ? 'rg-port' : 'rg-port nodrag'}
        data-side={side}
        data-kind={kind}
        data-connected={connected || undefined}
        data-drop={drop?.state}
        style={{ top }}
        title={drop?.state === 'refused' ? drop.reason : portTitle(side, kind)}
        {...events}
      >
        <PortDot kind={kind} />
      </Handle>
    );
  };

const Card = memo(function Card({ data }: NodeProps<CardNode>) {
  const { node, overlay, selected, connectedInputs, downstream, status, drops, write, menu } = data;
  return (
    <>
      <NodeCard
        family={node.family}
        name={node.id}
        impl={node.impl}
        param={node.param}
        inputs={node.inputs}
        output={node.output}
        connected={{ inputs: connectedInputs, output: downstream }}
        selected={selected}
        status={status}
        overlay={overlay}
        renderPort={handlePort(node.id, drops, write)}
      />
      {write === null ? null : (
        // A pointer's way to the menu Shift+F10 opens: the keyboard has that key, so this is no tab stop.
        <button type="button" className="rg-canvas__more nodrag" tabIndex={-1} aria-label={`More actions for ${node.id}`} onClick={(event) => {
            // The node's own click would close the menu this opens.
            event.stopPropagation();
            write.more(node.id);
          }}
        >
          <Glyph name="more" />
        </button>
      )}
      {menu}
    </>
  );
});

function Line({ sourceX, sourceY, targetX, targetY, source, target, data }: EdgeProps<LineEdge>) {
  return <EdgeLine x1={sourceX} y1={sourceY} x2={targetX} y2={targetY} from={source} to={target} port={data?.port ?? 0} kind={data?.kind ?? 'opaque'} invalid={data?.invalid ?? false} />;
}

// Stable across renders, as the canvas library requires.
const NODE_TYPES = { card: Card };
const EDGE_TYPES = { line: Line };

/**
 * Zoom and fit, over this canvas's own viewport. The percentage is read on
 * demand, never announced. Named after its canvas, so two toolbars side by
 * side are told apart.
 */
function Toolbar({ label, onFit }: { label: string; onFit: () => void }) {
  const flow = useReactFlow();
  const { zoom } = useViewport();
  const hidden = (text: string) => <span className="rg-visually-hidden">{text}</span>;
  return (
    <div className="rg-canvas__toolbar" role="toolbar" aria-label={`Canvas, ${label}`}>
      <Button kind="quiet" size="s" icon="minus" onClick={() => void flow.zoomOut()}>
        {hidden('Zoom out')}
      </Button>
      <span className="rg-canvas__zoom">{Math.round(zoom * 100)}%</span>
      <Button kind="quiet" size="s" icon="plus" onClick={() => void flow.zoomIn()}>
        {hidden('Zoom in')}
      </Button>
      <Button kind="quiet" size="s" icon="fit" onClick={onFit}>
        {hidden('Fit graph')}
      </Button>
    </div>
  );
}

/**
 * The library draws each grid as an `<svg>` and wraps each edge in one, and
 * takes no attribute for either. A grid says nothing: it is hidden. A wrapper
 * says nothing of its own and holds the edge, which is named: it is made
 * presentational, since hiding it would hide the edge's name with it.
 */
function quietLibrarySvgs(root: HTMLElement) {
  for (const grid of root.querySelectorAll('svg.react-flow__background')) {
    if (grid.getAttribute('aria-hidden') !== 'true') grid.setAttribute('aria-hidden', 'true');
  }
  for (const edge of root.querySelectorAll('.react-flow__edge')) {
    const wrapper = edge.parentElement;
    if (wrapper?.tagName.toLowerCase() === 'svg' && wrapper.getAttribute('role') !== 'none') wrapper.setAttribute('role', 'none');
  }
}

const NO_OVERLAY: Readonly<Record<string, NodeOverlay>> = {};
const NO_ISSUES: Readonly<Record<string, string>> = {};

/** An edge being drawn: from which node, and the port under the pointer, if any. */
type Drawing = { from: string; over: { node: string; port: number } | null; pointer: Position | null };

function Surface({
  graph,
  label,
  mode = 'read',
  layout,
  overlay = NO_OVERLAY,
  frame,
  selected: controlled,
  onSelect,
  onAutoPlaced,
  inspector,
  menu: entries,
  ports,
  refuse,
  onConnect,
  onMove,
  onInsert,
  onDropItem,
  issues = NO_ISSUES,
  invalidEdges,
}: CanvasProps) {
  const editable = EDITABLE[mode];
  const flowId = useId();
  const flow = useReactFlow();
  const root = useRef<HTMLDivElement>(null);
  const [own, setOwn] = useState<string | null>(null);
  const [menu, setMenu] = useState<string | null>(null);
  const [drawing, setDrawing] = useState<Drawing | null>(null);
  // Where a node is while the library drags it; handed to `onMove` on release.
  const [dragged, setDragged] = useState<Record<string, Position>>({});
  // Each card's measured size, handed back to the library on its node (below).
  const [measured, setMeasured] = useState<Record<string, { width: number; height: number }>>({});
  const [tick, setTick] = useState(0);
  const selected = controlled === undefined ? own : controlled;
  const select = (id: string | null) => {
    if (controlled === undefined) setOwn(id);
    onSelect?.(id);
  };

  const model = useMemo(() => {
    const drawn = toModel(graph);
    if (!editable || ports === undefined) return drawn;
    return { ...drawn, nodes: drawn.nodes.map((n) => (ports[n.id] === undefined ? n : { ...n, inputs: [...ports[n.id]!.inputs], output: ports[n.id]!.output })) };
  }, [graph, editable, ports]);
  // The graph and the stored positions only: replay data never moves a node.
  const resolved = useMemo(() => resolveLayout(model, layout), [model, layout]);

  // Reported once per distinct placement, not once per render.
  const placed = JSON.stringify(Object.fromEntries(resolved.autoPlaced.map((id) => [id, resolved.positions[id]!])));
  const report = useRef(onAutoPlaced);
  useEffect(() => {
    report.current = onAutoPlaced;
  });
  useEffect(() => {
    if (placed !== '{}') report.current?.(JSON.parse(placed) as Record<string, Position>);
  }, [placed]);

  // The library adds and replaces its SVGs as the graph changes, after this
  // component renders: they are set again whenever its subtree changes.
  useEffect(() => {
    const el = root.current;
    if (el === null) return;
    quietLibrarySvgs(el);
    const observer = new MutationObserver(() => quietLibrarySvgs(el));
    observer.observe(el, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  // The menu's node is passed back by the menu, so this closure holds no
  // render's state and is made once.
  const closeMenu = useCallback((id: string, refocus: boolean) => {
    setMenu(null);
    if (refocus) root.current?.querySelector<HTMLElement>(`.react-flow__node[data-id="${CSS.escape(id)}"]`)?.focus();
  }, []);

  // The write handlers read the latest props through a ref, so the object the
  // cards hold is made once and a card re-renders only for its own data.
  const latest = useRef({ refuse, onConnect });
  const drawingNow = useRef<Drawing | null>(null);
  drawingNow.current = drawing;
  useEffect(() => {
    latest.current = { refuse, onConnect };
  });
  const write: WriteHandlers | null = useMemo(
    () =>
      editable
        ? {
            start: (node) => setDrawing({ from: node, over: null, pointer: null }),
            enter: (node, port) => setDrawing((d) => (d === null ? d : { ...d, over: { node, port } })),
            leave: () => setDrawing((d) => (d === null ? d : { ...d, over: null })),
            // Read from the ref, never inside a state update, which StrictMode runs twice.
            drop: (node, port) => {
              const d = drawingNow.current;
              setDrawing(null);
              if (d !== null && latest.current.refuse?.(d.from, node, port) == null) latest.current.onConnect?.(d.from, node, port);
            },
            more: (node) => setMenu(node),
          }
        : null,
    [editable],
  );

  // A release anywhere but a port ends the drawing with nothing made; the
  // pointer is followed so the edge being drawn reaches it.
  const isDrawing = drawing !== null;
  useEffect(() => {
    if (!isDrawing) return;
    const end = () => setDrawing(null);
    const follow = (event: globalThis.PointerEvent) => setDrawing((d) => (d === null ? d : { ...d, pointer: flow.screenToFlowPosition({ x: event.clientX, y: event.clientY }) }));
    window.addEventListener('pointerup', end);
    window.addEventListener('pointermove', follow);
    return () => {
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointermove', follow);
    };
  }, [isDrawing, flow]);

  // Each replayed node's figures, said: the element its description points
  // at before the keys' description, which the library renders and names.
  const descriptions = useMemo(
    () => model.nodes.flatMap((node, i) => {
      const said = overlay[node.id] === undefined ? '' : describeOverlay(overlay[node.id]!);
      return said === '' ? [] : [{ node: node.id, id: `${flowId}-node-${i}`, said }];
    }),
    [model, overlay, flowId],
  );

  // In write mode the graph grows under the reader: each time a node comes
  // or goes the view is fitted again, at once, never above 100% — a fit of
  // one node would otherwise fill the canvas and leave the next off screen.
  // The library queues the fit until the new card is measured. The pane
  // resizing — the inspector opening beside it — fits again too, or the fit
  // would frame the graph in the width the pane had before.
  const count = model.nodes.length;
  const paneWidth = useStore((s) => s.width);
  const paneHeight = useStore((s) => s.height);
  useEffect(() => {
    if (editable) void flow.fitView(WRITE_FIT);
  }, [editable, count, paneWidth, paneHeight, flow]);

  // Read mode with a frame: the view is fitted to the frame from the graph's
  // corner, once the pane has a size and again whenever it or the frame
  // changes — not as the overlay changes, so the reader's pan and zoom stay
  // across queries.
  const framed = !editable && frame !== undefined;
  const bounds = useMemo(() => boundsOf(resolved.positions), [resolved]);
  const area: Box | null = framed ? fitArea(bounds, frame) : null;
  const [ax, ay, aw, ah] = area === null ? [0, 0, 0, 0] : [area.x, area.y, area.width, area.height];
  useEffect(() => {
    if (framed && paneWidth > 0 && paneHeight > 0) void flow.fitBounds({ x: ax, y: ay, width: aw, height: ah }, FRAME_FIT);
  }, [framed, ax, ay, aw, ah, paneWidth, paneHeight, flow]);
  const fit = () => void (area === null ? flow.fitView() : flow.fitBounds(area, FRAME_FIT));

  const positionOf = useCallback((id: string) => dragged[id] ?? resolved.positions[id]!, [dragged, resolved]);

  const nodes: CardNode[] = useMemo(() => {
    const downstream = new Set(model.edges.map((e) => e.from));
    const described = new Map(descriptions.map((d) => [d.node, d.id]));
    const size = nodeSize();
    const from = drawing?.from;
    return model.nodes.map((node) => {
      const isSelected = node.id === selected;
      const issue = editable ? issues[node.id] : undefined;
      const filled = new Set(model.edges.filter((e) => e.to === node.id).map((e) => e.port));
      const drops =
        from === undefined
          ? null
          : node.inputs.map((_, port): Drop => {
              const reason = refuse?.(from, node.id, port) ?? null;
              return reason === null ? { state: 'open' } : { state: 'refused', reason };
            });
      return {
        id: node.id,
        type: 'card',
        position: positionOf(node.id),
        ...(measured[node.id] === undefined ? {} : { measured: measured[node.id] }),
        initialWidth: size.width,
        initialHeight: size.height,
        handles: handles(node),
        draggable: editable,
        connectable: false,
        ariaLabel: accessibleName(node, isSelected, overlay[node.id], issue),
        ...(described.has(node.id) ? { domAttributes: { 'aria-describedby': `${described.get(node.id)!} ${KEYS_DESCRIPTION}-${flowId}` } } : {}),
        // The node whose menu is open is lifted above the others, so its menu is too.
        ...(menu === node.id ? { zIndex: 1 } : {}),
        data: {
          node,
          overlay: overlay[node.id],
          selected: isSelected,
          connectedInputs: node.inputs.map((_, port) => filled.has(port)),
          downstream: downstream.has(node.id),
          status: issue === undefined ? undefined : { kind: 'invalid', message: issue },
          drops,
          write,
          menu:
            menu === node.id ? (
              <NodeMenu node={node.id} onClose={(refocus) => closeMenu(node.id, refocus)}>
                {entries?.(node.id, () => closeMenu(node.id, true))}
              </NodeMenu>
            ) : null,
        },
      };
    });
    // `tick` hands the library fresh nodes when it asked for them (below).
  }, [tick, model, positionOf, measured, selected, overlay, menu, entries, editable, closeMenu, descriptions, flowId, issues, drawing?.from, refuse, write]);

  const edges: LineEdge[] = useMemo(() => {
    const flagged = new Set(editable ? (invalidEdges ?? []) : []);
    return model.edges.map((e) => ({
      id: e.id,
      type: 'line',
      source: e.from,
      target: e.to,
      sourceHandle: 'out',
      targetHandle: `in-${e.port}`,
      data: { port: e.port, kind: e.kind, invalid: flagged.has(e.id) },
    }));
  }, [model, editable, invalidEdges]);

  // The nodes are controlled: what the library changes comes back here. A
  // drag's positions and each card's measured size are kept — a size not
  // handed back on its node is forgotten when the node is next handed over,
  // and a fit then frames only the cards measured since — and anything else
  // (a fit's request for the nodes again) is answered with fresh nodes, which
  // is what lets the library run a queued fit.
  const onNodesChange = (changes: NodeChange<CardNode>[]) => {
    const moved = changes.flatMap((c) => (c.type === 'position' && c.position !== undefined ? [[c.id, c.position] as const] : []));
    if (editable && moved.length > 0) setDragged((d) => ({ ...d, ...Object.fromEntries(moved) }));
    const sized = changes.flatMap((c) => (c.type === 'dimensions' && c.dimensions !== undefined ? [[c.id, c.dimensions] as const] : []));
    if (sized.length > 0) setMeasured((m) => ({ ...m, ...Object.fromEntries(sized) }));
    if (changes.some((c) => c.type !== 'position' && c.type !== 'dimensions')) setTick((t) => t + 1);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    // The open menu handles its own keys, Escape included.
    if (event.key === 'Escape') {
      setDrawing(null);
      select(null);
      return;
    }
    if (editable && event.key === '/' && target.closest('[role="menu"]') === null) {
      event.preventDefault();
      onInsert?.();
      return;
    }
    const id = target.closest('.react-flow__node')?.getAttribute('data-id');
    if (id == null || target.closest('[role="menu"]') !== null) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      setMenu(null);
      select(id);
    } else if ((event.key === 'F10' && event.shiftKey) || event.key === 'ContextMenu') {
      event.preventDefault();
      setMenu(id);
    } else if (editable && ARROWS[event.key] !== undefined) {
      event.preventDefault();
      const [dx, dy] = ARROWS[event.key]!;
      const step = (event.shiftKey ? 4 : 1) * GRID;
      const at = positionOf(id);
      onMove?.(id, { x: snap(at.x) + dx * step, y: snap(at.y) + dy * step });
    }
  };

  const onDragOver = (event: DragEvent<HTMLDivElement>) => {
    if (!editable || !event.dataTransfer.types.includes(DROP_TYPE)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
  };
  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    if (!editable || !event.dataTransfer.types.includes(DROP_TYPE)) return;
    event.preventDefault();
    const at = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
    onDropItem?.(event.dataTransfer.getData(DROP_TYPE), { x: snap(at.x), y: snap(at.y) });
  };

  // What the status line says while an edge is drawn: the reason of the port under the pointer, or how to finish.
  const status = (() => {
    if (drawing === null) return '';
    const over = drawing.over === null ? null : (refuse?.(drawing.from, drawing.over.node, drawing.over.port) ?? null);
    return over ?? `Connecting from ${drawing.from}. Drop on an open port; Escape cancels.`;
  })();
  const source = drawing === null ? null : positionOf(drawing.from);

  const panel = selected !== null && inspector !== undefined ? inspector(selected) : null;
  return (
    <div className="rg-canvas-frame">
      <div ref={root} className="rg-canvas" data-mode={mode} data-drawing={drawing === null ? undefined : true} onKeyDown={onKeyDown} onDragOver={onDragOver} onDrop={onDrop}>
        {/* First in the tab order, before the nodes. */}
        <Toolbar label={label} onFit={fit} />
        {/* Its height is kept whether or not it speaks, so nothing moves when it does. */}
        {editable ? (
          <p className="rg-canvas__status" role="status">
            {status}
          </p>
        ) : null}
        <ReactFlow<CardNode, LineEdge>
          id={flowId}
          aria-label={label}
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          edgeTypes={EDGE_TYPES}
          nodesDraggable={editable}
          nodesConnectable={false}
          elementsSelectable={false}
          edgesFocusable={false}
          deleteKeyCode={null}
          selectionKeyCode={null}
          multiSelectionKeyCode={null}
          ariaLabelConfig={ARIA_LABELS[mode]}
          snapToGrid={editable}
          snapGrid={[GRID, GRID]}
          onNodesChange={onNodesChange}
          onNodeDragStop={(_, node) => {
            setDragged((d) => Object.fromEntries(Object.entries(d).filter(([k]) => k !== node.id)));
            onMove?.(node.id, { x: snap(node.position.x), y: snap(node.position.y) });
          }}
          fitView={!framed}
          {...(editable ? { fitViewOptions: WRITE_FIT } : {})}
          // The library's floor of 0.5 cannot fit a seven-column graph into a
          // half-width pane, the side-by-side case.
          minZoom={0.2}
          attributionPosition="bottom-right"
          onNodeClick={(_, node) => {
            setMenu(null);
            select(node.id);
          }}
          onNodeContextMenu={(event, node) => {
            event.preventDefault();
            setMenu(node.id);
          }}
          onPaneClick={() => {
            setMenu(null);
            select(null);
          }}
        >
          <Background id="minor" className="rg-canvas__grid" variant={BackgroundVariant.Dots} gap={16} size={1} />
          <Background id="major" className="rg-canvas__grid-major" variant={BackgroundVariant.Dots} gap={128} size={1.5} />
          {source === null || drawing?.pointer == null ? null : (
            <ViewportPortal>
              <svg className="rg-canvas__drawing" aria-hidden="true">
                <path className="rg-edge" data-drawing="true" d={edgePath(source.x + NODE_WIDTH + PORT_OVERHANG, source.y + portTop(0) + PORT / 2, drawing.pointer.x, drawing.pointer.y)} />
              </svg>
            </ViewportPortal>
          )}
        </ReactFlow>
        <Legend model={model} autoPlaced={resolved.autoPlaced.length} />
        <div hidden>
          {descriptions.map((d) => (
            <span key={d.id} id={d.id}>
              {d.said}
            </span>
          ))}
        </div>
      </div>
      {panel === null ? null : <div className="rg-canvas__inspector">{panel}</div>}
    </div>
  );
}

/**
 * The pipeline canvas: the lowered graph as node cards and typed edges, laid
 * out automatically or at stored positions, with pan, zoom, one selection and
 * a keyboard model — Tab walks the toolbar, then the nodes in topological
 * order; Enter selects, Shift+F10 opens the node menu, Escape clears. In
 * write mode nodes move, by drag or by the arrow keys, an edge is drawn from
 * an output port onto an input port that every port judges during the drag,
 * and `/` asks for the caller's insert list. It receives everything through
 * its props and makes no request; it knows no screen. Each canvas holds its
 * own viewport and selection, so two side by side share nothing.
 */
export function Canvas(props: CanvasProps) {
  return (
    <ReactFlowProvider>
      <Surface {...props} />
    </ReactFlowProvider>
  );
}
