import {
  Background,
  BackgroundVariant,
  Handle,
  Position as Side,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useViewport,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeHandle,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import type { Graph } from '../api/types.ts';
import { Button, FAMILY_LABEL } from '../../design/index.ts';
import { EdgeLine } from './Edge.tsx';
import { NODE_WIDTH, nodeSize, resolveLayout, type Position, type StoredLayout } from './layout.ts';
import { Legend } from './Legend.tsx';
import { toModel, type CanvasNode, type NodeOverlay, type PortKind } from './model.ts';
import { NodeCard } from './NodeCard.tsx';
import { NodeMenu } from './NodeMenu.tsx';
import { portTitle, portTop, type PortProps } from './Port.tsx';
import './Canvas.css';

export type CanvasProps = {
  /** The lowered graph, as `GET /runs/{id}` serves it. */
  graph: Graph;
  /** The canvas's accessible name, e.g. the pipeline's. */
  label: string;
  /**
   * What the canvas lets a person do. Read is its only mode today: pan, zoom,
   * select, open the menu; nothing that changes the graph.
   */
  mode?: 'read';
  /** Stored positions by node id. Absent, or missing a node, the automatic layout places it. */
  layout?: StoredLayout | undefined;
  /** Per node, what the caller knows of its execution for one query. */
  overlay?: Readonly<Record<string, NodeOverlay>> | undefined;
  /** The selected node. Pass it to control the selection; leave it out and the canvas keeps its own. */
  selected?: string | null;
  onSelect?: (id: string | null) => void;
  /** Called with the positions the automatic layout gave the nodes the stored layout did not name, so the caller can persist them. */
  onAutoPlaced?: (placed: Record<string, Position>) => void;
  /** What sits beside the canvas while a node is selected. */
  inspector?: (id: string) => ReactNode;
  /** The node menu's entries. */
  menu?: (id: string) => ReactNode;
};

type CardData = {
  node: CanvasNode;
  overlay: NodeOverlay | undefined;
  selected: boolean;
  menu: ReactNode | null;
};
type CardNode = Node<CardData, 'card'>;
type LineEdge = Edge<{ port: number; kind: PortKind }, 'line'>;

// Where a port's edge attaches: the port's outer side, at its centre. The
// canvas library reads these before it has measured a card, so an edge is
// right from the first frame, and measures the drawn ports afterwards.
const PORT = 10;
const PORT_OVERHANG = 6;
const handles = (node: CanvasNode): NodeHandle[] => [
  ...node.inputs.map((_, i) => ({ id: `in-${i}`, type: 'target' as const, position: Side.Left, x: -PORT_OVERHANG, y: portTop(i), width: PORT, height: PORT })),
  ...(node.output === null ? [] : [{ id: 'out', type: 'source' as const, position: Side.Right, x: NODE_WIDTH + PORT_OVERHANG - PORT, y: portTop(0), width: PORT, height: PORT }]),
];

function accessibleName(node: CanvasNode, selected: boolean, overlay: NodeOverlay | undefined): string {
  return [
    `${FAMILY_LABEL[node.family]} ${node.id}, ${node.impl}`,
    selected ? 'selected' : null,
    overlay?.error !== undefined ? 'failed' : null,
    overlay?.onlyHere ?? null,
  ]
    .filter((part) => part !== null)
    .join(', ');
}

const handlePort = ({ side, kind, index, top }: PortProps) => (
  <Handle
    key={`${side}-${index}`}
    id={side === 'in' ? `in-${index}` : 'out'}
    type={side === 'in' ? 'target' : 'source'}
    position={side === 'in' ? Side.Left : Side.Right}
    isConnectable={false}
    className="rg-port"
    data-side={side}
    data-kind={kind}
    data-connected="true"
    style={{ top }}
    title={portTitle(side, kind)}
  />
);

function Card({ data }: NodeProps<CardNode>) {
  const { node, overlay, selected, menu } = data;
  return (
    <>
      <NodeCard
        family={node.family}
        name={node.id}
        impl={node.impl}
        param={node.param}
        inputs={node.inputs}
        output={node.output}
        selected={selected}
        overlay={overlay}
        renderPort={handlePort}
      />
      {menu}
    </>
  );
}

function Line({ sourceX, sourceY, targetX, targetY, source, target, data }: EdgeProps<LineEdge>) {
  return <EdgeLine x1={sourceX} y1={sourceY} x2={targetX} y2={targetY} from={source} to={target} port={data?.port ?? 0} kind={data?.kind ?? 'opaque'} />;
}

// Stable across renders, as the canvas library requires.
const NODE_TYPES = { card: Card };
const EDGE_TYPES = { line: Line };

/** Zoom and fit, over this canvas's own viewport. */
function Toolbar() {
  const flow = useReactFlow();
  const { zoom } = useViewport();
  const hidden = (text: string) => <span className="rg-visually-hidden">{text}</span>;
  return (
    <div className="rg-canvas__toolbar" role="toolbar" aria-label="Canvas">
      <Button kind="quiet" size="s" icon="minus" onClick={() => void flow.zoomOut()}>
        {hidden('Zoom out')}
      </Button>
      <span className="rg-canvas__zoom" aria-live="polite">
        {Math.round(zoom * 100)}%
      </span>
      <Button kind="quiet" size="s" icon="plus" onClick={() => void flow.zoomIn()}>
        {hidden('Zoom in')}
      </Button>
      <Button kind="quiet" size="s" icon="fit" onClick={() => void flow.fitView()}>
        {hidden('Fit graph')}
      </Button>
    </div>
  );
}

const NO_OVERLAY: Readonly<Record<string, NodeOverlay>> = {};

function Surface({ graph, label, layout, overlay = NO_OVERLAY, selected: controlled, onSelect, onAutoPlaced, inspector, menu: entries }: CanvasProps) {
  const flowId = useId();
  const root = useRef<HTMLElement>(null);
  const [own, setOwn] = useState<string | null>(null);
  const [menu, setMenu] = useState<string | null>(null);
  const selected = controlled === undefined ? own : controlled;
  const select = (id: string | null) => {
    if (controlled === undefined) setOwn(id);
    onSelect?.(id);
  };

  const model = useMemo(() => toModel(graph), [graph]);
  const resolved = useMemo(() => resolveLayout(model, layout, overlay), [model, layout, overlay]);

  // Reported once per distinct placement, not once per render: a caller
  // passing a fresh overlay object each render is not told again.
  const placed = JSON.stringify(Object.fromEntries(resolved.autoPlaced.map((id) => [id, resolved.positions[id]!])));
  const report = useRef(onAutoPlaced);
  useEffect(() => {
    report.current = onAutoPlaced;
  });
  useEffect(() => {
    if (placed !== '{}') report.current?.(JSON.parse(placed) as Record<string, Position>);
  }, [placed]);

  const nodes: CardNode[] = model.nodes.map((node) => {
    const size = nodeSize(node, overlay[node.id]);
    const isSelected = node.id === selected;
    return {
      id: node.id,
      type: 'card',
      position: resolved.positions[node.id]!,
      initialWidth: size.width,
      initialHeight: size.height,
      handles: handles(node),
      ariaLabel: accessibleName(node, isSelected, overlay[node.id]),
      // The node whose menu is open is lifted above the others, so its menu is too.
      ...(menu === node.id ? { zIndex: 1 } : {}),
      data: {
        node,
        overlay: overlay[node.id],
        selected: isSelected,
        menu: menu === node.id ? <NodeMenu node={node.id}>{entries?.(node.id)}</NodeMenu> : null,
      },
    };
  });
  const edges: LineEdge[] = model.edges.map((e) => ({
    id: e.id,
    type: 'line',
    source: e.from,
    target: e.to,
    sourceHandle: 'out',
    targetHandle: `in-${e.port}`,
    data: { port: e.port, kind: e.kind },
  }));

  const focusNode = (id: string) => root.current?.querySelector<HTMLElement>(`.react-flow__node[data-id="${CSS.escape(id)}"]`)?.focus();
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (event.key === 'Escape') {
      if (menu !== null) {
        const id = menu;
        setMenu(null);
        focusNode(id);
      } else select(null);
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
    }
  };

  const panel = selected !== null && inspector !== undefined ? inspector(selected) : null;
  return (
    <div className="rg-canvas-frame">
      <section ref={root} className="rg-canvas" aria-label={label} onKeyDown={onKeyDown}>
        <ReactFlow<CardNode, LineEdge>
          id={flowId}
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          edgeTypes={EDGE_TYPES}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          edgesFocusable={false}
          deleteKeyCode={null}
          selectionKeyCode={null}
          multiSelectionKeyCode={null}
          fitView
          // The library's floor of 0.5 cannot fit a seven-column graph into a
          // half-width pane, the side-by-side case.
          minZoom={0.2}
          proOptions={{ hideAttribution: true }}
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
        </ReactFlow>
        <Toolbar />
        <Legend model={model} autoPlaced={resolved.autoPlaced.length} />
      </section>
      {panel === null ? null : <div className="rg-canvas__inspector">{panel}</div>}
    </div>
  );
}

/**
 * The pipeline canvas: the lowered graph as node cards and typed edges, laid
 * out automatically or at stored positions, with pan, zoom, one selection and
 * a keyboard model — Tab walks the nodes in topological order, Enter selects,
 * Shift+F10 opens the node menu, Escape clears. It receives everything through
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
