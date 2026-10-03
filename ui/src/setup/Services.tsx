// The Services section: one row per binding `workspace.toml` holds — family
// and name, the address, what the last probe read and its status — with Test
// (the probe `bench` runs before a run, ADR-C32 § 4) and Remove; ADR-C32's
// rule under the list; and the Connect form.
import { useId, type Ref } from 'react';
import { Button, InlineMessage, Input, Section, StatusChip, type Status } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { ServiceBinding, ServiceStatus } from '../api/types.ts';
import { ErrorState, Resource, type RequestState } from '../shell/states.tsx';
import { ConnectForm, type ConnectFormProps } from './forms.tsx';
import { serviceKey } from './model.ts';

/** ADR-C32's rule in one sentence, as issue #345 words it for this screen. */
export const ADDRESS_RULE = 'The address never enters a pipeline. A run records which address answered, as provenance — two runs with different addresses and the same identity are one experiment run twice.';

/** A probe this page ran: at which address, when, and what came back. */
export type SessionProbe = { uri: string; at: Date; outcome: { ok: true; identity: string } | { ok: false; problem: ApiProblem } };

type RowStatus =
  | { kind: 'connected'; identity: string | null; at: Date | null }
  | { kind: 'unreachable' | 'refused'; problem: ApiProblem; at: Date }
  | { kind: 'untested' };

/**
 * A row's status. A probe this page ran at the binding's current address
 * decides it; otherwise the server's own memory does — `connected` when its
 * last probe at this address read an identity — with no time, since the
 * server keeps none.
 */
export function rowStatus(service: ServiceStatus, probe: SessionProbe | undefined): RowStatus {
  if (probe !== undefined && probe.uri === service.uri) {
    if (probe.outcome.ok) return { kind: 'connected', identity: probe.outcome.identity, at: probe.at };
    return { kind: probe.outcome.problem.code === 'service_unreachable' ? 'unreachable' : 'refused', problem: probe.outcome.problem, at: probe.at };
  }
  return service.connected ? { kind: 'connected', identity: service.identity, at: null } : { kind: 'untested' };
}

const CHIP: Record<RowStatus['kind'], Exclude<Status, 'running'>> = { connected: 'done', unreachable: 'failed', refused: 'warning', untested: 'queued' };

const clock = (at: Date) => (
  <time dateTime={at.toISOString()}>{at.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</time>
);

/**
 * What a probe this page ran says while it runs and once it read an identity,
 * in the row's status line, which is always present so a screen reader
 * announces both. A refusal is the row's `ErrorState` instead, beside it.
 */
function Outcome({ testing, status, uri }: { testing: boolean; status: RowStatus; uri: string }) {
  if (testing) {
    return (
      <InlineMessage tone="info" title="Testing…">
        Reading the identity a run would record, at <code>{uri}</code>.
      </InlineMessage>
    );
  }
  if (status.kind === 'connected' && status.at !== null) {
    return (
      <InlineMessage tone="info" title={`Connected · ${status.identity ?? 'no identity reported'}`}>
        The identity a run on this binding would record.
      </InlineMessage>
    );
  }
  return null;
}

type RowProps = {
  service: ServiceStatus;
  probe: SessionProbe | undefined;
  testing: boolean;
  servedModel: string;
  onServedModel: (value: string) => void;
  onTest: () => void;
  onRemove: () => void;
  rowRef: (el: HTMLLIElement | null) => void;
};

function ServiceRow({ service, probe, testing, servedModel, onServedModel, onTest, onRemove, rowRef }: RowProps) {
  const key = serviceKey(service);
  const status = rowStatus(service, probe);
  const field = `service-model-${key.replace(/[^A-Za-z0-9_-]/g, '_')}`;
  const refused = !testing && (status.kind === 'unreachable' || status.kind === 'refused') ? status.problem : null;
  return (
    <li className="rg-setup__service" aria-label={key} tabIndex={-1} ref={rowRef} aria-busy={testing || undefined}>
      <div className="rg-setup__service-head">
        <span className="rg-setup__name">{key}</span>
        <code className="rg-setup__address">{service.uri}</code>
        {/* Last on its line: the chip's word changes with the probe, and nothing after it moves. */}
        <StatusChip state={CHIP[status.kind]}>{status.kind}</StatusChip>
      </div>
      <p className="rg-setup__identity">
        {status.kind === 'connected' ? (
          <>
            Identity <code>{status.identity ?? 'not reported'}</code>
            {' · '}
            {status.at === null ? 'read before this page was opened' : <>read at {clock(status.at)}</>}
          </>
        ) : status.kind === 'untested' ? (
          'Not tested at this address since this page was opened.'
        ) : (
          <>Tested at {clock(status.at)}.</>
        )}
      </p>
      <div role="status" className="rg-setup__outcome">
        <Outcome testing={testing} status={status} uri={service.uri} />
      </div>
      {refused === null ? null : <ErrorState problem={refused} />}
      <div className="rg-setup__service-actions">
        <Input id={field} label="Served model" aria-label={`Served model for ${key}`} mono value={servedModel} onChange={(e) => onServedModel(e.target.value)} />
        <Button size="s" busy={testing} onClick={onTest} aria-label={`Test ${key}`}>
          Test
        </Button>
        <Button size="s" kind="destructive" onClick={onRemove} aria-label={`Remove ${key}`}>
          Remove
        </Button>
      </div>
    </li>
  );
}

/**
 * A binding removed from the list in this page, holding its row's place —
 * where the row was, as tall as it was — so the rows below never move.
 * `pending` while Undo can still bring it back and nothing is written;
 * `writing` once the window closed and `DELETE` is in flight; `removed` once
 * the server unbound it.
 */
export type RemovedSlot = { binding: ServiceBinding; index: number; height: number; state: 'pending' | 'writing' | 'removed' };

function Removed({ slot, undoId, onUndo }: { slot: RemovedSlot; undoId: string; onUndo: () => void }) {
  const messageId = useId();
  const key = serviceKey(slot.binding);
  return (
    // Height from the row it replaces, set through the DOM's style object, which the page's content security policy allows.
    <li className="rg-setup__service" aria-label={`${key}, removed`} style={{ minHeight: slot.height }}>
      <div id={messageId}>
        <InlineMessage
          tone="info"
          title={`Removed ${key}.`}
          action={
            slot.state === 'pending' ? (
              <Button size="s" id={undoId} onClick={onUndo} aria-describedby={messageId}>
                Undo
              </Button>
            ) : undefined
          }
        >
          It was bound to <code>{slot.binding.uri}</code>.
        </InlineMessage>
      </div>
    </li>
  );
}

/** The list as drawn: the rows, minus every binding a slot stands for, with each slot at its place. */
export function displayed(services: readonly ServiceStatus[], slots: readonly RemovedSlot[]): (ServiceStatus | RemovedSlot)[] {
  const taken = new Set(slots.map((s) => serviceKey(s.binding)));
  const rows: (ServiceStatus | RemovedSlot)[] = services.filter((s) => !taken.has(serviceKey(s)));
  for (const slot of slots) rows.splice(Math.min(slot.index, rows.length), 0, slot);
  return rows;
}

const isSlot = (row: ServiceStatus | RemovedSlot): row is RemovedSlot => 'binding' in row;

export type ServicesProps = {
  state: RequestState<readonly ServiceStatus[]>;
  onRetry: () => void;
  /** A binding being connected, drawn as its row from the click until the server lists it. */
  connecting: ServiceStatus | null;
  probes: ReadonlyMap<string, SessionProbe>;
  testing: ReadonlySet<string>;
  models: ReadonlyMap<string, string>;
  onServedModel: (key: string, value: string) => void;
  onTest: (service: ServiceStatus) => void;
  onRemove: (service: ServiceStatus) => void;
  slots: readonly RemovedSlot[];
  /** A removal the server refused once its window closed; the row is back. */
  refusal: ApiProblem | null;
  onUndo: (slot: RemovedSlot) => void;
  /** The pending Undo's id, so focus can land on it once the row it replaces is gone. */
  undoId: string;
  rowRef: (key: string) => (el: HTMLLIElement | null) => void;
  connect: ConnectFormProps;
  anchor: Ref<HTMLElement>;
};

export function Services({ state, onRetry, connecting, probes, testing, models, onServedModel, onTest, onRemove, slots, refusal, onUndo, undoId, rowRef, connect, anchor }: ServicesProps) {
  return (
    <Section heading="Services" caption="A service is connected when its identity was read, not when a port answered." anchor={anchor}>
      <Resource state={state} loading="Reading services" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(listed) => {
          const services = connecting !== null && !listed.some((s) => serviceKey(s) === serviceKey(connecting)) ? [...listed, connecting] : listed;
          const rows = displayed(services, slots);
          return rows.length === 0 ? (
            <p className="rg-setup__note">No service is bound: a pipeline runs only on the components this build carries until one is.</p>
          ) : (
            <ul className="rg-setup__services">
              {rows.map((row) => {
                if (isSlot(row)) {
                  return <Removed key={`removed:${serviceKey(row.binding)}`} slot={row} undoId={undoId} onUndo={() => onUndo(row)} />;
                }
                const key = serviceKey(row);
                const busy = connecting !== null && serviceKey(connecting) === key;
                return (
                  <ServiceRow
                    key={key}
                    service={row}
                    probe={probes.get(key)}
                    testing={busy || testing.has(key)}
                    servedModel={models.get(key) ?? ''}
                    onServedModel={(value) => onServedModel(key, value)}
                    onTest={() => (busy ? undefined : onTest(row))}
                    onRemove={() => (busy ? undefined : onRemove(row))}
                    rowRef={rowRef(key)}
                  />
                );
              })}
            </ul>
          );
        }}
      </Resource>
      {refusal === null ? null : <ErrorState problem={refusal} />}
      <p className="rg-setup__note">{ADDRESS_RULE}</p>
      <p className="rg-setup__note">The times shown are those of probes run since this page was opened; the server keeps none. A served model typed on a row is kept for this page only.</p>
      <h3 className="rg-setup__subheading">Connect a service</h3>
      <ConnectForm {...connect} />
    </Section>
  );
}
