import type { ReactNode } from 'react';
import { FamilyTile, Glyph, Progress, RankStrip, type Family } from '../../design/index.ts';
import { percent, type NodeOverlay, type PortKind } from './model.ts';
import { PortMark, portTop, type PortProps } from './Port.tsx';
import './NodeCard.css';

/** What the node is doing, said in its head; the two failures also say why inside the card. */
export type NodeStatus =
  | { kind: 'invalid'; message: string }
  | { kind: 'failed'; message: string }
  | { kind: 'running'; value: number; total: number; label: string }
  | { kind: 'queued' }
  | { kind: 'not-run' };

export type NodeCardProps = {
  family: Family;
  name: string;
  /** The implementation line, in mono. */
  impl: string;
  /** The one key parameter, shown at rest; replay drops it for the result. */
  param?: { name: string; value: string } | undefined;
  /** One input port per entry, in port order. */
  inputs?: readonly PortKind[];
  output?: PortKind | null;
  /** Which ports an edge meets: drawn filled — every input, or each by port. Neither, by default. */
  connected?: { inputs: boolean | readonly boolean[]; output: boolean };
  selected?: boolean;
  status?: NodeStatus | undefined;
  /** A drop target that is not a node yet, or a node this build cannot run. */
  variant?: 'ghost' | 'unavailable';
  dragging?: boolean;
  /** Its execution for one query. Present, the card is in replay: ports go solid and the body shows the result. */
  overlay?: NodeOverlay | undefined;
  /** Draws a port; the canvas passes its own so an edge can attach. A plain mark by default. */
  renderPort?: (port: PortProps) => ReactNode;
  /** The preview page's way to show hover or focus at rest. */
  previewState?: 'hover' | 'focus';
};


/**
 * A pipeline node: the family tile with its glyph, the name, the
 * implementation and one key parameter, its ports typed by shape. Every state
 * the design system defines is a prop, drawn from a data attribute; the card
 * knows no screen and no canvas library.
 */
export function NodeCard({
  family,
  name,
  impl,
  param,
  inputs = [],
  output = null,
  connected = { inputs: false, output: false },
  selected = false,
  status,
  variant,
  dragging = false,
  overlay,
  renderPort = (p) => <PortMark key={`${p.side}-${p.index}`} {...p} />,
  previewState,
}: NodeCardProps) {
  const shown: NodeStatus | undefined = overlay?.error !== undefined ? { kind: 'failed', message: overlay.error } : overlay?.notRun === true ? { kind: 'not-run' } : status;
  return (
    <div
      className="rg-node"
      data-family={family}
      data-selected={selected || undefined}
      data-status={shown?.kind}
      data-variant={variant}
      data-dragging={dragging || undefined}
      data-replay={overlay === undefined ? undefined : true}
      data-only-here={overlay?.onlyHere === undefined ? undefined : true}
      data-preview-state={previewState}
      aria-hidden={variant === 'ghost' || undefined}
    >
      <div className="rg-node__head">
        <FamilyTile family={family} />
        <span className="rg-node__title">
          <b>{name}</b>
          <span>{impl}</span>
        </span>
        {overlay?.onlyHere !== undefined ? <span className="rg-node__tag">{overlay.onlyHere}</span> : <State status={shown} />}
      </div>
      {overlay === undefined ? (
        param === undefined ? null : (
          <div className="rg-node__param">
            <span>{param.name}</span>
            <span>{param.value}</span>
          </div>
        )
      ) : shown?.kind === 'not-run' ? null : (
        <Replay overlay={overlay} />
      )}
      {shown?.kind === 'running' ? (
        <div className="rg-node__progress">
          <Progress state="running" value={shown.value} total={shown.total} label={shown.label} />
        </div>
      ) : null}
      {shown?.kind === 'invalid' || shown?.kind === 'failed' ? (
        <div className="rg-node__msg">
          <Glyph name="alert" />
          <span>{shown.message}</span>
        </div>
      ) : null}
      {inputs.map((kind, index) => renderPort({ side: 'in', kind, index, top: portTop(index), connected: typeof connected.inputs === 'boolean' ? connected.inputs : (connected.inputs[index] ?? false) }))}
      {output === null ? null : renderPort({ side: 'out', kind: output, index: 0, top: portTop(0), connected: connected.output })}
    </div>
  );
}

function State({ status }: { status: NodeStatus | undefined }) {
  if (status === undefined) return null;
  const glyph = status.kind === 'queued' ? 'clock' : status.kind === 'running' || status.kind === 'not-run' ? null : 'alert';
  return (
    <span className="rg-node__state">
      {glyph === null ? null : <Glyph name={glyph} />}
      {status.kind === 'not-run' ? 'not run' : status.kind}
    </span>
  );
}

function Replay({ overlay }: { overlay: NodeOverlay }) {
  const { metric, ranks, discarded, durationMs, share } = overlay;
  const rows = [metric, ranks, discarded, durationMs, share].some((v) => v !== undefined);
  if (!rows) return null;
  const title = durationMs !== undefined && share !== undefined ? `${durationMs} ms, ${percent(share)} of this query's time` : undefined;
  return (
    <div className="rg-node__replay">
      {metric === undefined ? null : (
        <div className="rg-node__metric">
          <span>{metric.name}</span>
          <b>{metric.value}</b>
        </div>
      )}
      {ranks === undefined ? null : (
        <div className="rg-node__metric">
          <span>top 10</span>
          <RankStrip hits={ranks} />
        </div>
      )}
      {discarded === undefined ? null : <div className="rg-node__note">{discarded} discarded</div>}
      {durationMs === undefined && share === undefined ? null : (
        <div className="rg-node__dur" title={title}>
          {share === undefined ? <span /> : (
            <i>
              <b style={{ width: `${Math.max(2, Math.round(share * 100))}%` }} />
            </i>
          )}
          <span>{durationMs === undefined ? '' : `${durationMs} ms`}</span>
        </div>
      )}
    </div>
  );
}
