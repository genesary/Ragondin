// The Compare screen: a baseline and up to four runs of one benchmark, read
// from `POST /compare` (the front-end design, § 3). Its state is the address,
// `#compare/<id>+<id>…?baseline=<id>`; it owns nothing else but what is shown
// open. ARCHITECTURE.md § The Compare screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { CompareRequest, Comparison, Pairing, RunListing } from '../api/types.ts';
import type { PairOutcome } from './PairingPanel.tsx';
import { formatHash, navigate, type Route } from '../routes.ts';
import { addressIds, resolveAddressIds } from '../runs/model.ts';
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
  return <Resolving client={client} ids={ids} baseline={known} />;
}

/**
 * The runs the address names, read back to their full ids: an address writes
 * a run by its 12-character prefix where that names one run (`addressIds`),
 * so every id waits for `GET /runs` before anything is compared. One the
 * listing cannot resolve — or a listing that fails — is compared as written,
 * and the API says what it names.
 */
function Resolving({ client, ids, baseline }: { client: ApiClient; ids: readonly string[]; baseline: string }) {
  const [listing, setListing] = useState<RequestState<RunListing>>({ status: 'loading' });
  const readListing = useCallback(async () => {
    setListing({ status: 'loading' });
    const result = await client.get('/runs');
    setListing(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [client]);
  useEffect(() => {
    void readListing();
  }, [readListing]);

  const known = useMemo(() => (listing.status === 'loaded' ? listing.value.runs.map((r) => r.id) : []), [listing]);
  // Every id waits for the listing, whatever its shape: the listing, not a
  // guess at what an id looks like, says which run a prefix names.
  const waiting = listing.status === 'loading';
  const [full, fullBaseline] = useMemo(() => {
    const resolved = resolveAddressIds([...ids, baseline], known);
    return [resolved.slice(0, -1), resolved.at(-1) ?? baseline] as const;
  }, [ids, baseline, known]);
  if (waiting) {
    return (
      <Sheet>
        <Loading label={`Comparing ${runs(ids.length)}`} />
      </Sheet>
    );
  }
  return <Comparing client={client} ids={full} baseline={fullBaseline} listing={listing} onRetryListing={() => void readListing()} known={known} />;
}

/** The key of one comparison: what the address names. */
const keyOf = (ids: readonly string[], baseline: string) => `${ids.join('+')}?${baseline}`;

type Read = { state: RequestState<Comparison>; key: string; shown: Comparison | null };

type ComparingProps = {
  client: ApiClient;
  /** The runs compared, by their full ids. */
  ids: readonly string[];
  baseline: string;
  listing: RequestState<RunListing>;
  onRetryListing: () => void;
  /** Every run the listing holds, by id: what an address's prefixes are unique among. */
  known: readonly string[];
};

function Comparing({ client, ids, baseline, listing, onRetryListing, known }: ComparingProps) {
  const key = keyOf(ids, baseline);
  const [read, setRead] = useState<Read>({ state: { status: 'loading' }, key, shown: null });
  const [texts, setTexts] = useState<ReadonlyMap<string, string> | null>(null);
  const textsAsked = useRef(false);
  // Answers already in hand, by key: a run added or a pairing kept was
  // compared before the address moved, and is not asked for twice.
  const answered = useRef(new Map<string, Comparison>());
  const latest = useRef(0);
  // The comparison read in flight — null once it settles — cancelled once a
  // newer one overtakes it. A pairing's post is never held here: it writes,
  // and the API may have kept it whether or not its answer still lands.
  const reading = useRef<AbortController | null>(null);
  const supersede = () => {
    reading.current?.abort();
    reading.current = null;
  };
  // The address as it stands now: an answer that arrives later builds on
  // this, never on the ids and baseline it captured when it was asked.
  const request = useRef<CompareRequest>({ run_ids: [...ids], baseline });
  request.current = { run_ids: [...ids], baseline };

  const compare = useCallback(async () => {
    const mine = ++latest.current;
    supersede();
    const body = request.current;
    const asked = keyOf(body.run_ids, body.baseline);
    const ready = answered.current.get(asked);
    if (ready !== undefined) {
      setRead({ state: { status: 'loaded', value: ready }, key: asked, shown: ready });
      return;
    }
    setRead((prev) => ({ state: { status: 'loading' }, key: asked, shown: prev.state.status === 'loaded' ? prev.state.value : prev.shown }));
    const controller = new AbortController();
    reading.current = controller;
    const result = await client.post('/compare', body, { signal: controller.signal });
    if (reading.current === controller) reading.current = null;
    // Only the answer to the last comparison asked for lands. Both checks
    // drop an overtaken one: whatever cancels a read also moves the count,
    // and the signal alone covers the screen going, which moves nothing.
    if (mine !== latest.current || controller.signal.aborted) return;
    if (result.ok) answered.current.set(asked, result.value);
    setRead((prev) => ({ state: result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem }, key: asked, shown: result.ok ? result.value : prev.shown }));
  }, [client]);

  useEffect(() => {
    void compare();
  }, [compare, key]);
  // The screen going cancels the read it leaves behind.
  useEffect(() => supersede, []);

  // The address of a comparison, each run by its prefix where that names one
  // run among the listing's and the runs compared.
  const addressOf = useCallback(
    (runIds: readonly string[], base: string): Route => {
      const among = [...new Set([...known, ...runIds])];
      const [b = base, ...rest] = addressIds([base, ...runIds], among);
      return { screen: 'compare', ids: rest, baseline: b };
    },
    [known],
  );

  // A query's text is the dataset's, the same in every run of the benchmark:
  // read once, from the baseline, the first time a bar's list opens.
  const onWantTexts = useCallback(() => {
    if (textsAsked.current) return;
    textsAsked.current = true;
    void client.get('/runs/{id}/queries', { id: request.current.baseline }).then((result) => {
      if (result.ok) setTexts(new Map(result.value.queries.flatMap((q) => (q.text === null ? [] : [[q.id, q.text] as const]))));
    });
  }, [client]);

  const onAdd = async (id: string): Promise<ApiProblem | null> => {
    const asked = { run_ids: [...request.current.run_ids, id], baseline: request.current.baseline };
    const result = await client.post('/compare', asked);
    if (!result.ok) return result.problem;
    answered.current.set(keyOf(asked.run_ids, asked.baseline), result.value);
    // The address may have moved while the run was being compared — a new
    // baseline, a run removed: the run joins the address as it stands now.
    const now = request.current;
    navigate(addressOf(now.run_ids.includes(id) ? now.run_ids : [...now.run_ids, id], now.baseline));
    return null;
  };

  const onPair = async (pairing: Pairing): Promise<PairOutcome> => {
    // A pairing's answer is a comparison like any other: it lands only if
    // nothing newer was asked for meanwhile.
    const mine = ++latest.current;
    // A comparison read this pairing overtakes is cancelled; should the
    // pairing then be refused, nothing would land, so it is asked again.
    const overtook = reading.current !== null;
    supersede();
    const body = { ...request.current, pairing };
    const asked = keyOf(body.run_ids, body.baseline);
    const result = await client.post('/compare', body);
    // An answer means the API kept the pairing, whether or not the answer
    // still lands: every answer in hand predates it and is never served again.
    if (result.ok) answered.current.clear();
    if (mine !== latest.current) {
      // The newer comparison on screen, or in flight, may have been read
      // before the pairing was written: ask again for the address as it stands.
      if (result.ok) void compare();
      return { kind: 'superseded', kept: result.ok };
    }
    if (!result.ok) {
      if (overtook) void compare();
      return { kind: 'refused', problem: result.problem };
    }
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
      onRetryListing={onRetryListing}
      onAdd={onAdd}
      onPair={onPair}
      addressOf={addressOf}
      texts={texts}
      onWantTexts={onWantTexts}
    />
  );
}
