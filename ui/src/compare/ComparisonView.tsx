// One comparison, loaded: the run bar, the four charts with their tables, the
// metric table, the stages with "Pair nodes…", the parameter matrix, and the
// verdict that ends the page with its one action. The front-end design, § 3;
// ARCHITECTURE.md § The Compare screen.
import { useId, useState } from 'react';
import {
  BarChart,
  Button,
  ButtonLink,
  ChartFrame,
  FAMILY_LABEL,
  FamilyTile,
  Histogram,
  InlineMessage,
  LineChart,
  RunSwatch,
  Section,
  SegmentedControl,
  Select,
  Sheet,
  StackedBarChart,
  type LegendItem,
  type RunSeries,
} from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { Comparison, Pairing, RunListing } from '../api/types.ts';
import { formatHash, type Route } from '../routes.ts';
import type { RequestState } from '../shell/states.tsx';
import { defaultMetric } from '../metrics.ts';
import { barMetrics, binsOf, deltaOf, hasChange, latencyBars, pairsByHandLabel, noVerdict, queriesByDelta, regressions, runSeries, stageLabel, stageLine, stageMetrics, unplacedLabel, verdict } from './model.ts';
import { PairingPanel, type PairOutcome } from './PairingPanel.tsx';
import { RunBar } from './RunBar.tsx';
import { BinsTable, LatencyTable, metricsCaption, MetricsTable, ParameterMatrix, STAGES_CAPTION, StageTable } from './tables.tsx';

export type ComparisonViewProps = {
  comparison: Comparison;
  ids: readonly string[];
  baseline: string;
  /** A newer comparison is being read; this one stays on screen meanwhile. */
  busy: boolean;
  listing: RequestState<RunListing>;
  onRetryListing: () => void;
  onAdd: (id: string) => Promise<ApiProblem | null>;
  onPair: (pairing: Pairing) => Promise<PairOutcome>;
  /** The address of a comparison of `ids` against `baseline`, its runs as an address writes them. */
  addressOf: (ids: readonly string[], baseline: string) => Route;
  /** Each query's text by id, once read; null before. */
  texts: ReadonlyMap<string, string> | null;
  /** Asks for the queries' texts, the first time a bar's list opens. */
  onWantTexts: () => void;
};

/** The run legend every run chart carries: each run's swatch and its name in words. */
const runLegend = (series: readonly RunSeries[]): LegendItem[] =>
  series.map((s) => ({
    id: s.id,
    label: s.label,
    mark: (
      <span aria-hidden="true">
        <RunSwatch slot={s.ink} small />
      </span>
    ),
  }));

const ms = (v: number) => `${Number.isInteger(v) ? v : v.toFixed(1)} ms`;
const four = (v: number) => v.toFixed(4);
const queries = (n: number) => `${n.toLocaleString('en-US')} quer${n === 1 ? 'y' : 'ies'}`;

export function ComparisonView({ comparison: c, ids, baseline, busy, listing, onRetryListing, onAdd, onPair, addressOf, texts, onWantTexts }: ComparisonViewProps) {
  const series = runSeries(c);
  const listId = useId();
  const pairingId = useId();
  const [pairing, setPairing] = useState(false);
  const [stageMetric, setStageMetric] = useState<string | null>(null);
  const [focusRun, setFocusRun] = useState<string | null>(null);
  const [queryMetric, setQueryMetric] = useState<string | null>(null);
  const [openBin, setOpenBin] = useState<string | null>(null);

  // The bars: the metrics read on one 0–1 scale.
  const bars = barMetrics(c.metrics);
  const barIndex = (row: number, s: number) => (bars[row]?.best ?? []).includes(c.runs[s]?.id ?? '');

  // The stage line, for one metric the stages carry.
  const stageNames = stageMetrics(c);
  // Every stage figure is a ranking metric, so no family is passed: ndcg@10, else the first.
  const metricAtStages = stageMetric !== null && stageNames.includes(stageMetric) ? stageMetric : defaultMetric(stageNames, {});
  const line = metricAtStages === null ? null : stageLine(c, metricAtStages);
  const lowConfidence = c.stages.filter((row) => row.confidence === 'low');

  // The latency stack, by family.
  const latency = latencyBars(c);
  const familyLegend: LegendItem[] = [
    ...latency.families.map((f) => ({ id: f, label: FAMILY_LABEL[f], mark: <FamilyTile family={f} /> })),
    ...latency.others.map((word) => ({ id: word, label: word, mark: <span className="rg-chart__key" data-tone="zero" aria-hidden="true" /> })),
  ];

  // The histogram and the verdict: one run against the baseline, on one metric.
  const others = c.runs.slice(1);
  const runAt = Math.max(1, c.runs.findIndex((r) => r.id === focusRun));
  const run = c.runs[runAt] ?? c.runs[1];
  const runName = series[runAt]?.label ?? '';
  const deltas = c.query_deltas.find((d) => d.run === run?.id)?.metrics ?? [];
  // Every per-query delta is of a ranking metric, so no family is passed:
  // the stage line's rule, ndcg@10, else the first.
  const shownMetric = deltas.some((m) => m.metric === queryMetric) ? queryMetric : defaultMetric(deltas.map((m) => m.metric), {});
  const md = deltas.find((m) => m.metric === shownMetric) ?? null;
  const bins = md === null ? [] : binsOf(md);
  const open = md?.bins.find((b) => b.bin === openBin) ?? null;
  const openBinView = bins.find((b) => b.id === openBin) ?? null;
  const deltaOfQuery = new Map((md?.deltas ?? []).map((d) => [d.query, d.delta]));
  const replayFor = (id: string, query: string) => formatHash({ screen: 'replay', run: id, query, with: baseline });
  const replayOf = (query: string) => replayFor(run?.id ?? '', query);
  // The verdict: one sentence for every run against the baseline, each on the
  // histogram's metric where that run has it — the run the histogram shows
  // holds the page's one primary action.
  const verdicts = others.map((other, i) => {
    const index = i + 1;
    const name = series[index]?.label ?? '';
    const its = c.query_deltas.find((d) => d.run === other.id)?.metrics ?? [];
    const metric = its.some((m) => m.metric === shownMetric) ? shownMetric : defaultMetric(its.map((m) => m.metric), {});
    const deltasOf = its.find((m) => m.metric === metric) ?? null;
    return {
      id: other.id,
      name,
      sentence: deltasOf === null ? noVerdict(c, name) : verdict(deltasOf, name),
      metric: deltasOf?.metric ?? null,
      lost: deltasOf === null ? null : regressions(deltasOf),
      changed: deltasOf === null || hasChange(deltasOf),
      primary: other.id === run?.id,
    };
  });
  const benchmark = c.ground_truth.benchmark;
  const openBins = (id: string) => {
    onWantTexts();
    setOpenBin((b) => (b === id ? null : id));
  };

  return (
    // Every section shows the comparison on screen, which a newer one is
    // replacing while busy: the whole sheet says so, not only the run bar.
    <div className="rg-compare" aria-busy={busy || undefined}>
      <Sheet>
        <Section heading="Runs compared">
          <p className="rg-compare__note">
            On {benchmark === null ? 'dataset ' : 'the benchmark '}
            <strong>{benchmark ?? c.ground_truth.expected.dataset_version.slice(0, 12)}</strong>, each run against the baseline.
          </p>
          {/* Always present, so a screen reader hears its text change both ways. */}
          <p className="rg-compare__busy" role="status">
            {busy ? 'Comparing again…' : ''}
          </p>
          <RunBar comparison={c} ids={ids} baseline={baseline} listing={listing} onRetryListing={onRetryListing} onAdd={onAdd} addressOf={addressOf} />
        </Section>

        <Section heading="Metrics" caption="The best of each row in bold; each delta against the baseline, by sign and word.">
          <div className="rg-compare__stack">
            {bars.length === 0 ? null : (
              <ChartFrame
                caption="Each metric per run, on one 0–1 scale"
                legend={runLegend(series)}
                tableBelow={metricsCaption(c)}
              >
                <BarChart
                  label="Each metric per run"
                  groups={bars.map((b) => ({ id: b.name, label: b.name }))}
                  series={series}
                  values={bars.map((b) => b.values)}
                  domain={[0, 1]}
                  format={four}
                  best={barIndex}
                />
              </ChartFrame>
            )}
            <MetricsTable comparison={c} series={series} />
          </div>
        </Section>

        <Section heading="Stages" caption={pairsByHandLabel(c)}>
          <div className="rg-compare__stack">
            <div className="rg-compare__pickers">
              {stageNames.length === 0 ? null : (
                <Select
                  id="compare-stage-metric"
                  label="Stage metric"
                  value={metricAtStages ?? ''}
                  options={stageNames.map((m) => ({ value: m, label: m }))}
                  onChange={(e) => setStageMetric(e.target.value)}
                />
              )}
              {/* The panel is rendered only while open, so it is named only then: a reference to an absent element is invalid. */}
              <Button aria-expanded={pairing} aria-controls={pairing ? pairingId : undefined} onClick={() => setPairing((p) => !p)}>
                Pair nodes…
              </Button>
            </div>
            {lowConfidence.length === 0 ? null : (
              <InlineMessage tone="warning" title={`The automatic pairing is a guess at ${lowConfidence.map((r) => stageLabel(r.stage)).join(' and ')}`}>
                A pipeline&apos;s graph is ambiguous there. If the stages below compare the wrong nodes, pair them by hand.
              </InlineMessage>
            )}
            {/* Every pair a run could not place, in words (decided in #402): drawn with the answer, so it moves nothing later.
                The live region is always present, empty or not, so a list a re-compare brings or changes is announced. */}
            <div className="rg-compare__unplaced" role="status" aria-label="Pairs drawn by hand not placed">
              {c.unplaced_pairs.length === 0 ? null : (
                <>
                  <InlineMessage
                    tone="info"
                    title={`${c.unplaced_pairs.length} pair${c.unplaced_pairs.length === 1 ? '' : 's'} drawn by hand not placed: a run lacks the node`}
                  />
                  <ul aria-label="Pairs not placed">
                    {c.unplaced_pairs.map((u) => (
                      <li key={`${u.run}-${u.pair.node}-${u.pair.other}`}>{unplacedLabel(c, u)}</li>
                    ))}
                  </ul>
                </>
              )}
            </div>
            {pairing ? <PairingPanel id={pairingId} comparison={c} onPair={onPair} /> : null}
            {line === null || metricAtStages === null ? (
              <p className="rg-compare__note">No stage carries a figure: {c.ground_truth.detail}.</p>
            ) : (
              <ChartFrame
                caption={`${metricAtStages} at each stage`}
                note="A run without a stage has no point there: the line breaks rather than cross it."
                legend={runLegend(series)}
                tableBelow={STAGES_CAPTION}
              >
                <LineChart label={`${metricAtStages} at each stage`} x={line.x} series={series} values={line.values} dots={line.dots} domain={[0, 1]} format={four} gapLabel={line.gap} />
              </ChartFrame>
            )}
            <StageTable comparison={c} series={series} metric={metricAtStages ?? ''} />
          </div>
        </Section>

        <Section heading="Latency per node" caption="The median over the queries, each node by its family.">
          {familyLegend.length === 0 ? (
            <p className="rg-compare__note">No node of these runs recorded a duration.</p>
          ) : (
            <ChartFrame caption="Median latency per node, in milliseconds" legend={familyLegend} table={<LatencyTable bars={latency.bars} segments={latency.segments} format={ms} />}>
              <StackedBarChart label="Median latency per node" bars={latency.bars} segments={latency.segments} format={ms} />
            </ChartFrame>
          )}
        </Section>

        <Section heading="Per query" caption="Each query's change against the baseline, in seven bins from much worse to much better. Open a bar for its queries.">
          <div className="rg-compare__stack">
            <div className="rg-compare__pickers">
              {others.length < 2 ? null : (
                <SegmentedControl
                  label="Run"
                  options={others.map((r, i) => ({ value: r.id, label: series[i + 1]?.label ?? r.id }))}
                  value={run?.id ?? ''}
                  onChange={(v) => {
                    setFocusRun(v);
                    setOpenBin(null);
                  }}
                />
              )}
              {deltas.length < 2 ? null : (
                <Select
                  id="compare-query-metric"
                  label="Per-query metric"
                  value={md?.metric ?? ''}
                  options={deltas.map((m) => ({ value: m.metric, label: m.metric }))}
                  onChange={(e) => {
                    setQueryMetric(e.target.value);
                    setOpenBin(null);
                  }}
                />
              )}
            </div>
            {md === null ? (
              <p className="rg-compare__note">{noVerdict(c, runName)}</p>
            ) : (
              <>
                <ChartFrame
                  caption={`Per-query change in ${md.metric}, ${runName} against the baseline`}
                  note={`${queries(md.judged_queries)} judged and scored in both runs.`}
                  legend={[
                    { id: 'worse', label: 'worse', mark: <span className="rg-chart__key" data-tone="worse" aria-hidden="true" /> },
                    { id: 'zero', label: 'unchanged', mark: <span className="rg-chart__key" data-tone="zero" aria-hidden="true" /> },
                    { id: 'better', label: 'better', mark: <span className="rg-chart__key" data-tone="better" aria-hidden="true" /> },
                  ]}
                  table={<BinsTable caption={`Per-query change in ${md.metric}, ${runName}, as a table`} bins={bins} />}
                >
                  <Histogram
                    label={`Per-query change in ${md.metric}, ${runName} against the baseline`}
                    bins={bins}
                    halves={{ worse: 'worse', better: 'better' }}
                    active={openBin}
                    onActivate={openBins}
                    controls={listId}
                  />
                </ChartFrame>
                <section id={listId} className="rg-compare__queries" aria-label={open === null || openBinView === null ? 'Queries of a bin' : `${queries(open.count)} ${openBinView.label}, ${openBinView.range}`} hidden={open === null}>
                  {open === null || openBinView === null ? null : (
                    <>
                      <h3>
                        {queries(open.count)} {openBinView.label}, {openBinView.range}
                      </h3>
                      {open.queries.length === 0 ? (
                        <p className="rg-compare__note">No query falls in this bin.</p>
                      ) : (
                        <ul>
                          {queriesByDelta(open.queries, md.deltas).map((q) => {
                            const d = deltaOfQuery.get(q);
                            return (
                              <li key={q}>
                                <a href={replayOf(q)}>
                                  <span className="rg-compare__query-id">{q}</span>
                                  <span className="rg-compare__query-text">{texts?.get(q) ?? ''}</span>
                                  <span className="rg-compare__query-delta">{d === undefined ? '' : deltaOf('higher', d).text}</span>
                                </a>
                              </li>
                            );
                          })}
                        </ul>
                      )}
                    </>
                  )}
                </section>
              </>
            )}
          </div>
        </Section>

        <Section heading="Configuration" caption="Only the parameters not identical across the runs; departures from the baseline marked.">
          <ParameterMatrix configuration={c.configuration} series={series} />
        </Section>

        <Section heading="Verdict">
          {verdicts.map((v) => (
            <div key={v.id} className="rg-compare__judged">
              <p className="rg-compare__verdict">{v.sentence}</p>
              <div className="rg-compare__action">
                {v.metric !== null && v.lost !== null && v.lost.worst !== null ? (
                  <>
                    <ButtonLink kind={v.primary ? 'primary' : 'secondary'} size={v.primary ? 'l' : 'm'} icon="play" href={formatHash({ screen: 'replay', run: v.id, query: v.lost.worst, with: baseline, set: { kind: 'regressions', metric: v.metric } })}>
                      {`Replay the ${v.lost.count.toLocaleString('en-US')} regression${v.lost.count === 1 ? '' : 's'}`}
                    </ButtonLink>
                    <span className="rg-compare__note">Opens the largest regression, {v.lost.worst}, beside the baseline, with the others one step away.</span>
                  </>
                ) : v.changed ? (
                  <ButtonLink kind={v.primary ? 'primary' : 'secondary'} size={v.primary ? 'l' : 'm'} icon="play" href={formatHash({ screen: 'replay', run: v.id })}>
                    {`Open ${v.name} in Replay`}
                  </ButtonLink>
                ) : (
                  <span className="rg-compare__note">No query changed, so there is nothing to replay.</span>
                )}
              </div>
            </div>
          ))}
        </Section>
      </Sheet>
    </div>
  );
}
