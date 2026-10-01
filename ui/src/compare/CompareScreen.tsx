// The Compare screen: a baseline and up to four runs of one benchmark, read
// from `POST /compare` (the front-end design, § 3). Its state is the address,
// `#compare/<id>+<id>…?baseline=<id>`; it owns nothing else but what is shown
// open. ARCHITECTURE.md § The Compare screen.
import { useCallback, useEffect, useRef, useState } from 'react';
import { ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { CompareRequest, Comparison, Pairing, RunListing } from '../api/types.ts';
import type { PairOutcome } from './PairingPanel.tsx';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { ComparisonView } from './ComparisonView.tsx';
import './Compare.css';

export type CompareScreenProps = {
  client: ApiClient;
  /** The runs the address names, in its order. */
  ids: readonly string[];
  /** The baseline the address names, if it names one. */
  baseline?: string | undefined;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function CompareScreen({ client, ids, baseline }: CompareScreenProps) {
  const known = baseline !== undefined && ids.includes(baseline) ? baseline : undefined;
  const first = ids[0];

  useEffect(() => {
    // An address without a baseline, or naming one it does not compare, is
    // corrected in place to the first run: a default filled in, not a move.
    if (ids.length >= 2 && known === undefined && first !== undefined) navigate({ screen: 'compare', ids: [...ids], baseline: first }, { replace: true });
  }, [ids, known, first]);

  if (ids.length < 2) {
    return (
      <Sheet>
        <EmptyState
          heading="Choose at least two runs"
          action={
            <ButtonLink kind="primary" size="l" href={formatHash(ids.length === 0 ? { screen: 'runs' } : { screen: 'runs', sel: [...ids] })}>
              Open Runs
            </ButtonLink>
          }
        >
          A comparison is a baseline and up to four runs of one benchmark, chosen in Runs.
        </EmptyState>
      </Sheet>
    );
  }
  if (known === undefined) return null;
  return <Comparing client={client} ids={ids} baseline={known} />;
}

/** The key of one comparison: what the address names. */
const keyOf = (ids: readonly string[], baseline: string) => `${ids.join('+')}?${baseline}`;

type Read = { state: RequestState<Comparison>; key: string; shown: Comparison | null };

function Comparing({ client, ids, baseline }: { client: ApiClient; ids: readonly string[]; baseline: string }) {
  const key = keyOf(ids, baseline);
  const [read, setRead] = useState<Read>({ state: { status: 'loading' }, key, shown: null });
  const [listing, setListing] = useState<RequestState<RunListing>>({ status: 'loading' });
  // Answers already in hand, by key: a run added or a pairing kept was
  // compared before the address moved, and is not asked for twice.
  const answered = useRef(new Map<string, Comparison>());
  const latest = useRef(0);
  // The address as it stands now: an answer that arrives later builds on
  // this, never on the ids and baseline it captured when it was asked.
  const request = useRef<CompareRequest>({ run_ids: [...ids], baseline });
  request.current = { run_ids: [...ids], baseline };

  const compare = useCallback(async () => {
    const mine = ++latest.current;
    const body = request.current;
    const asked = keyOf(body.run_ids, body.baseline);
    const ready = answered.current.get(asked);
    if (ready !== undefined) {
      setRead({ state: { status: 'loaded', value: ready }, key: asked, shown: ready });
      return;
    }
    setRead((prev) => ({ state: { status: 'loading' }, key: asked, shown: prev.state.status === 'loaded' ? prev.state.value : prev.shown }));
    const result = await client.post('/compare', body);
    // Only the answer to the last comparison asked for lands.
    if (mine !== latest.current) return;
    if (result.ok) answered.current.set(asked, result.value);
    setRead((prev) => ({ state: result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem }, key: asked, shown: result.ok ? result.value : prev.shown }));
  }, [client]);

  useEffect(() => {
    void compare();
  }, [compare, key]);

  const readListing = useCallback(async () => {
    setListing({ status: 'loading' });
    const result = await client.get('/runs');
    setListing(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [client]);

  useEffect(() => {
    void readListing();
  }, [readListing]);

  const onAdd = async (id: string): Promise<ApiProblem | null> => {
    const asked = { run_ids: [...request.current.run_ids, id], baseline: request.current.baseline };
    const result = await client.post('/compare', asked);
    if (!result.ok) return result.problem;
    answered.current.set(keyOf(asked.run_ids, asked.baseline), result.value);
    // The address may have moved while the run was being compared — a new
    // baseline, a run removed: the run joins the address as it stands now.
    const now = request.current;
    navigate({ screen: 'compare', ids: now.run_ids.includes(id) ? now.run_ids : [...now.run_ids, id], baseline: now.baseline });
    return null;
  };

  const onPair = async (pairing: Pairing): Promise<PairOutcome> => {
    // A pairing's answer is a comparison like any other: it lands only if
    // nothing newer was asked for meanwhile.
    const mine = ++latest.current;
    const body = { ...request.current, pairing };
    const asked = keyOf(body.run_ids, body.baseline);
    const result = await client.post('/compare', body);
    if (mine !== latest.current) return { kind: 'superseded' };
    if (!result.ok) return { kind: 'refused', problem: result.problem };
    // A kept pairing changes the answer for these runs: what disk holds now.
    answered.current.clear();
    answered.current.set(asked, result.value);
    setRead({ state: { status: 'loaded', value: result.value }, key: asked, shown: result.value });
    return { kind: 'kept' };
  };

  const { state, shown } = read;
  if (state.status === 'error') {
    if (state.problem.code === 'runs_not_comparable') {
      return (
        <Sheet>
          <EmptyState
            heading="These runs cannot be compared"
            action={
              <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'runs', sel: [...ids] })}>
                Open Runs
              </ButtonLink>
            }
          >
            <span>{state.problem.message}</span> <span>{state.problem.hint}</span>
          </EmptyState>
        </Sheet>
      );
    }
    return <ErrorState problem={state.problem} onRetry={() => void compare()} />;
  }
  const value = state.status === 'loaded' ? state.value : shown;
  if (value === null) {
    return (
      <Sheet>
        <Loading label={`Comparing ${runs(ids.length)}`} />
      </Sheet>
    );
  }
  return (
    <ComparisonView
      comparison={value}
      ids={ids}
      baseline={baseline}
      busy={state.status === 'loading'}
      listing={listing}
      onRetryListing={() => void readListing()}
      onAdd={onAdd}
      onPair={onPair}
    />
  );
}
