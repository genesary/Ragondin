// The Services section: one row per binding `workspace.toml` holds — family
// and name, the address, what the last probe read and its status — with Test
// (the probe `bench` runs before a run, ADR-C32 § 4) and Remove; ADR-C32's
// rule under the list; and the Connect form.
import type { Ref } from 'react';
import { Button, InlineMessage, Input, Section, StatusChip, type Status } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { ServiceBinding, ServiceStatus } from '../api/types.ts';
import { ErrorState, Resource, type RequestState } from '../shell/states.tsx';
import { ConnectForm, type ConnectFormProps } from './forms.tsx';
import { serviceKey } from './model.ts';

/** ADR-C32's rule, verbatim from the front-end design, § 3. */
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
 * The outcome of a probe this page ran, in one slot of about one message's
 * height whatever it holds — testing, connected, or the refusal — so the
 * answer, which arrives long after the click, replaces the testing message
 * rather than pushing the rows below it down. A row this page has not tested
 * has no slot.
 */
function Outcome({ testing, status, uri }: { testing: boolean; status: RowStatus; uri: string }) {
  if (testing) {
    return (
      <InlineMessage tone="info" title="Testing…">
        Reading the identity a run would record, at <code>{uri}</code>.
      </InlineMessage>
    );
  }
  if (status.kind === 'unreachable' || status.kind === 'refused') return <ErrorState problem={status.problem} />;
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
      <Outcome testing={testing} status={status} uri={service.uri} />
      <div className="rg-setup__service-actions">
        <Input id={field} label="Served model" mono value={servedModel} onChange={(e) => onServedModel(e.target.value)} />
        <Button size="s" busy={testing} onClick={onTest}>
          Test
        </Button>
        <Button size="s" kind="destructive" onClick={onRemove}>
          Remove
        </Button>
      </div>
    </li>
  );
}

/**
 * The last removal this page made. Once the binding is gone, `slot` holds
 * its place in the list — where its row was, as tall as the row was — for the
 * message that offers Undo, so the rows below never move when the answer
 * arrives. A refused removal has no slot; its refusal is said below the list.
 */
export type Removal = { binding: ServiceBinding; slot: { index: number; height: number } | null; problem: ApiProblem | null };

function RemovedSlot({ removal, undoId, onUndo, height }: { removal: Removal; undoId: string; onUndo: () => void; height: number }) {
  return (
    // Height from the row it replaces, set through the DOM's style object, which the page's content security policy allows.
    <li className="rg-setup__service" aria-label={`${serviceKey(removal.binding)}, removed`} style={{ minHeight: height }}>
      <InlineMessage
        tone="info"
        title={`Removed ${serviceKey(removal.binding)}.`}
        action={
          <Button size="s" id={undoId} onClick={onUndo}>
            Undo
          </Button>
        }
      >
        It was bound to <code>{removal.binding.uri}</code>.
      </InlineMessage>
      {removal.problem === null ? null : <ErrorState problem={removal.problem} />}
    </li>
  );
}

/** The rows, with the removed binding's slot where its row was. */
function withSlot(services: readonly ServiceStatus[], removal: Removal | null): (ServiceStatus | 'removed')[] {
  const rows: (ServiceStatus | 'removed')[] = [...services];
  if (removal?.slot != null) rows.splice(Math.min(removal.slot.index, rows.length), 0, 'removed');
  return rows;
}

export type ServicesProps = {
  state: RequestState<readonly ServiceStatus[]>;
  onRetry: () => void;
  probes: ReadonlyMap<string, SessionProbe>;
  testing: ReadonlySet<string>;
  models: ReadonlyMap<string, string>;
  onServedModel: (key: string, value: string) => void;
  onTest: (service: ServiceStatus) => void;
  onRemove: (service: ServiceStatus) => void;
  removal: Removal | null;
  onUndo: (binding: ServiceBinding) => void;
  /** The Undo button's id, so focus can land on it once the row it replaces is gone. */
  undoId: string;
  rowRef: (key: string) => (el: HTMLLIElement | null) => void;
  connect: ConnectFormProps;
  anchor: Ref<HTMLElement>;
};

export function Services({ state, onRetry, probes, testing, models, onServedModel, onTest, onRemove, removal, onUndo, undoId, rowRef, connect, anchor }: ServicesProps) {
  return (
    <Section heading="Services" caption="A service is connected when its identity was read, not when a port answered." anchor={anchor}>
      <Resource state={state} loading="Reading services" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(services) =>
          services.length === 0 && removal?.slot == null ? (
            <p className="rg-setup__note">No service is bound: a pipeline here runs on this build’s own components.</p>
          ) : (
            <ul className="rg-setup__services">
              {withSlot(services, removal).map((s) => {
                if (s === 'removed') {
                  return removal?.slot == null ? null : <RemovedSlot key="removed" removal={removal} undoId={undoId} height={removal.slot.height} onUndo={() => onUndo(removal.binding)} />;
                }
                const key = serviceKey(s);
                return (
                  <ServiceRow
                    key={key}
                    service={s}
                    probe={probes.get(key)}
                    testing={testing.has(key)}
                    servedModel={models.get(key) ?? ''}
                    onServedModel={(value) => onServedModel(key, value)}
                    onTest={() => onTest(s)}
                    onRemove={() => onRemove(s)}
                    rowRef={rowRef(key)}
                  />
                );
              })}
            </ul>
          )
        }
      </Resource>
      <p className="rg-setup__note">{ADDRESS_RULE}</p>
      <p className="rg-setup__note">The times shown are those of probes run since this page was opened; the server keeps none.</p>
      {removal !== null && removal.slot === null && removal.problem !== null ? <ErrorState problem={removal.problem} /> : null}
      <h3 className="rg-setup__subheading">Connect a service</h3>
      <ConnectForm {...connect} />
    </Section>
  );
}
