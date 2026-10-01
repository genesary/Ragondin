import { useState, type ReactNode } from 'react';
import {
  Button,
  Checkbox,
  Delta,
  EmptyState,
  FAMILIES,
  FAMILY_LABEL,
  FamilyTile,
  FilterChip,
  GLYPH_NAMES,
  Glyph,
  InlineMessage,
  Input,
  Inspector,
  MetricChip,
  Progress,
  RankStrip,
  RunSwatch,
  Section,
  SegmentedControl,
  Select,
  Sheet,
  StatusChip,
  StatusDot,
  Table,
  Tabs,
  Toast,
  TopBar,
  type ButtonKind,
} from '../index.ts';
import { HYBRID_RERANK_GEN } from '../../src/canvas/fixtures.ts';
import { Canvas, NodeCard, type NodeCardProps } from '../../src/canvas/index.ts';
import './preview.css';

/** Every primitive the preview shows; its test holds this to the list the design system commits. */
export const COMPONENTS = [
  'Glyph', 'Button', 'Input', 'Select', 'Checkbox', 'StatusChip', 'MetricChip', 'FilterChip', 'RunSwatch', 'Table',
  'Sheet', 'Inspector', 'Toast', 'InlineMessage', 'Progress', 'EmptyState', 'RankStrip', 'SegmentedControl', 'Tabs', 'TopBar',
  'StatusDot', 'NodeCard', 'Canvas',
] as const;

const KINDS: ButtonKind[] = ['primary', 'secondary', 'quiet', 'destructive'];
const HASH = '9e2b7d41c0a3f5e6';

function Block({ name, children }: { name: (typeof COMPONENTS)[number]; children: ReactNode }) {
  return (
    <section className="rg-preview__block">
      <h2>{name}</h2>
      {children}
    </section>
  );
}

const Caption = ({ children }: { children: ReactNode }) => <span className="rg-preview__caption">{children}</span>;

function Buttons() {
  return (
    <div className="rg-preview__grid">
      <span />
      {['rest', 'hover', 'focus', 'pressed', 'disabled', 'loading'].map((s) => (
        <Caption key={s}>{s}</Caption>
      ))}
      {KINDS.map((kind) => (
        <FragmentRow key={kind} label={kind}>
          <Button kind={kind}>Launch run</Button>
          <Button kind={kind} data-preview-state="hover">
            Launch run
          </Button>
          <Button kind={kind} data-preview-state="focus">
            Launch run
          </Button>
          <Button kind={kind} data-preview-state="pressed">
            Launch run
          </Button>
          <Button kind={kind} disabled disabledReason="Select runs on one benchmark to compare">
            Launch run
          </Button>
          <Button kind={kind} busy busyLabel="Launching">
            Launch run
          </Button>
        </FragmentRow>
      ))}
    </div>
  );
}

function FragmentRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <Caption>{label}</Caption>
      {children}
    </>
  );
}

const RERANKER: NodeCardProps = {
  family: 'reranker',
  name: 'reranked',
  impl: 'reranker/cross_encoder',
  param: { name: 'top_k', value: '10' },
  inputs: ['query', 'chunks'],
  output: 'chunks',
};

/** Every state the design system defines for a node card. */
const CARDS: [string, NodeCardProps][] = [
  ['default', { ...RERANKER, family: 'retriever', name: 'vectors', impl: 'retriever/dense', inputs: ['query'] }],
  ['hover', { ...RERANKER, previewState: 'hover' }],
  ['keyboard focus', { ...RERANKER, previewState: 'focus' }],
  ['selected', { ...RERANKER, selected: true }],
  ['invalid', { ...RERANKER, param: { name: 'top_k', value: '0' }, status: { kind: 'invalid', message: 'top_k must be at least 1. Launch waits for this fix.' } }],
  ['running', { family: 'generator', name: 'answer', impl: 'generator/answerer', inputs: ['query', 'context'], output: 'answer', status: { kind: 'running', value: 3982, total: 10570, label: '3,982 / 10,570' } }],
  ['queued', { family: 'context', name: 'prompt', impl: 'context_builder/concat', param: { name: 'max_chunks', value: '5' }, inputs: ['query', 'chunks'], output: 'context', status: { kind: 'queued' } }],
  ['dragging', { family: 'fusion', name: 'fused', impl: 'fusion/rrf', param: { name: 'k', value: '60' }, inputs: ['chunks', 'chunks'], output: 'chunks', dragging: true }],
  ['ghost drop target', { ...RERANKER, name: 'Drop to add', variant: 'ghost', output: null }],
  ['not in this build', { family: 'generator', name: 'local-llama', impl: 'not in this build', inputs: ['context'], output: 'answer', variant: 'unavailable' }],
  ['query input', { family: 'query', name: 'question', impl: 'pipeline input', output: 'query' }],
  ['control flow (neutral)', { family: 'control', name: 'branch', impl: 'extension/branch', inputs: ['chunks'], output: 'opaque' }],
  ['replay', { ...RERANKER, overlay: { metric: { name: 'nDCG@10', value: '0.861' }, ranks: [1, 2], discarded: 90, durationMs: 349, share: 0.85 } }],
  ['replay, selected', { ...RERANKER, family: 'fusion', name: 'fused', impl: 'fusion/rrf', selected: true, overlay: { metric: { name: 'nDCG@10', value: '0.647' }, ranks: [2, 3], durationMs: 1, share: 0.01 } }],
  ['replay, failed', { ...RERANKER, overlay: { error: 'The service at 127.0.0.1:7001 did not answer within 30 s.' } }],
  ['replay, absent from the other run', { ...RERANKER, family: 'retriever', name: 'lexical', impl: 'retriever/bm25', inputs: ['query'], overlay: { onlyHere: 'only in B', ranks: [3, 7], durationMs: 9, share: 0.02 } }],
];

const REPLAY = {
  lexical: { ranks: [3, 7], durationMs: 9, share: 0.02 },
  vectors: { ranks: [2, 5], durationMs: 31, share: 0.08 },
  fused: { ranks: [2, 3], durationMs: 1, share: 0.01 },
  reranked: { metric: { name: 'nDCG@10', value: '0.861' }, ranks: [1, 2], discarded: 90, durationMs: 349, share: 0.85 },
};

/** Stateful demos keep their own state per theme column. */
function useDemoState() {
  const [mode, setMode] = useState('single');
  const [tab, setTab] = useState('benchmarks');
  const [pressed, setPressed] = useState(true);
  const [checked, setChecked] = useState(true);
  return { mode, setMode, tab, setTab, pressed, setPressed, checked, setChecked };
}

function Column({ theme }: { theme: 'light' | 'dark' }) {
  const s = useDemoState();
  const id = (name: string) => `${theme}-${name}`;
  return (
    <div className="rg-preview__theme" data-theme={theme}>
      <h1>{theme === 'light' ? 'Light' : 'Dark'}</h1>

      <Block name="Glyph">
        <div className="rg-preview__row">
          {GLYPH_NAMES.map((g) => (
            <Glyph key={g} name={g} label={g} />
          ))}
        </div>
        <div className="rg-preview__row">
          {FAMILIES.map((f) => (
            <span key={f} className="rg-preview__row">
              <FamilyTile family={f} />
              <Caption>{FAMILY_LABEL[f]}</Caption>
            </span>
          ))}
        </div>
      </Block>

      <Block name="Button">
        <Buttons />
      </Block>

      <Block name="Input">
        <Input id={id('rest')} label="top_k" defaultValue="100" help="1 to 5,183" numeric />
        <Input id={id('hover')} label="top_k (hover)" defaultValue="100" data-preview-state="hover" numeric />
        <Input id={id('focus')} label="path (focus)" defaultValue="~/ragondin-ws" data-preview-state="focus" mono />
        <Input id={id('invalid')} label="top_k (invalid)" defaultValue="5000" numeric error="5000 is more than the corpus holds. Use 1 to 5,183." />
        <Input id={id('unit')} label="timeout" defaultValue="30" unit="s" numeric />
        <Input id={id('readonly')} label="embedder (read-only)" defaultValue="bge-small-en-v1.5" readOnly mono />
        <Input id={id('disabled')} label="seed (disabled)" defaultValue="7" disabled />
      </Block>

      <Block name="Select">
        <Select id={id('sel')} label="Benchmark" options={[{ value: 'scifact', label: 'beir/scifact' }, { value: 'fiqa', label: 'beir/fiqa' }]} />
        <Select id={id('sel-inv')} label="Benchmark (invalid)" options={[{ value: 'fiqa', label: 'beir/fiqa' }]} error="beir/fiqa is not downloaded." />
        <Select id={id('sel-dis')} label="Benchmark (disabled)" options={[{ value: 'scifact', label: 'beir/scifact' }]} disabled />
      </Block>

      <Block name="Checkbox">
        <Checkbox label="hybrid-rerank" checked={s.checked} onChange={s.setChecked} />
        <Checkbox label="dense-only" checked={false} onChange={() => {}} />
        <Checkbox label="All runs on beir/scifact" checked={false} indeterminate onChange={() => {}} />
        <Checkbox label="squad/dev" checked={false} onChange={() => {}} disabled disabledReason="another benchmark, can't join this comparison" />
        <Checkbox label="beir/scifact" accessibleLabel="Select run 5b77e3a1c9f0 on beir/scifact" checked={false} onChange={() => {}} />
      </Block>

      <Block name="StatusChip">
        <div className="rg-preview__row">
          <StatusChip state="queued">queued, 2 ahead</StatusChip>
          <StatusChip state="running" fraction={0.38} />
          <StatusChip state="done" />
          <StatusChip state="warning">slow: p95 2.1 s</StatusChip>
          <StatusChip state="failed">
            failed at <code>rerank</code>
          </StatusChip>
          <StatusChip state="cancelled" />
        </div>
      </Block>

      <Block name="MetricChip">
        <div className="rg-preview__row">
          <MetricChip name="nDCG@10" value="0.6483" />
          <MetricChip name="nDCG@10" value="0.7217" best delta={<Delta meaning="better" direction="up">+0.073</Delta>} />
          <MetricChip name="p50" value="412 ms" delta={<Delta meaning="worse" direction="up">+351 ms</Delta>} />
          <MetricChip name="EM" value="41.2" delta={<Delta meaning="same" direction="none">0.0</Delta>} />
        </div>
      </Block>

      <Block name="FilterChip">
        <div className="rg-preview__row">
          <FilterChip label="beir/scifact" count={12} pressed={s.pressed} onToggle={s.setPressed} />
          <FilterChip label="done" count={9} pressed={false} onToggle={() => {}} />
          <FilterChip label="beir/fiqa" pressed={false} onToggle={() => {}} disabled disabledReason="not downloaded" />
        </div>
      </Block>

      <Block name="RunSwatch">
        <RunSwatch slot="base" name="dense-only" hash="4a01c9e7d2b3" />
        <RunSwatch slot="a" name="hybrid" hash="5b77e3a1c9f0" />
        <RunSwatch slot="b" name="hybrid-rerank" hash={HASH} />
        <RunSwatch slot="c" name="bm25-only" hash="c3d0a9e1f5b2" />
        <RunSwatch slot="d" name="colbert" hash="d41e8f2a7c6b" />
        <div className="rg-preview__row">
          <RunSwatch slot="base" name="dense-only" small />
          <RunSwatch slot="a" name="hybrid" small />
          <RunSwatch slot="b" name="hybrid-rerank" small />
        </div>
      </Block>

      <Block name="Table">
        <Table
          caption="Metrics of three runs"
          columns={[
            { id: 'metric', label: 'Metric' },
            { id: 'base', label: <RunSwatch slot="base" name="dense-only" small />, numeric: true },
            { id: 'a', label: <RunSwatch slot="a" name="hybrid-rerank" small />, numeric: true },
          ]}
          rows={[
            { kind: 'group', id: 'ranking', label: 'Ranking' },
            { id: 'ndcg', cells: ['nDCG@10', '0.6483', <>0.7217<Delta meaning="better" direction="up">+0.073</Delta></>], bestColumn: 2 },
            { id: 'recall', cells: ['Recall@100', '0.9120', '0.9050'], bestColumn: 1, selected: true },
            { kind: 'group', id: 'cost', label: 'Cost' },
            { id: 'p50', cells: ['p50 latency', '61 ms', <>412 ms<Delta meaning="worse" direction="up">+351 ms</Delta></>], bestColumn: 1 },
          ]}
        />
        <Table
          caption="Runs of two pipelines"
          columns={[
            { id: 'bench', label: 'Benchmark' },
            { id: 'run', label: 'Run' },
            { id: 'status', label: 'Status' },
          ]}
          onOpen={() => {}}
          onToggle={() => {}}
          rows={[
            {
              kind: 'group',
              id: 'hybrid',
              label: (
                <>
                  <a href="#pipeline-preview">hybrid-rerank</a> <FamilyTile family="retriever" labelled /> <FamilyTile family="fusion" labelled /> <FamilyTile family="reranker" labelled />
                </>
              ),
            },
            { id: 'r1', label: 'Run 9e2b7d41c0a3 on beir/scifact', cells: [<Checkbox key="c" label="beir/scifact" checked onChange={() => {}} tabIndex={-1} />, HASH.slice(0, 12), <StatusChip key="s" state="done" />] },
            { id: 'q', passive: true, cells: ['', '', <StatusChip key="s" state="queued" />] },
            { kind: 'group', id: 'dense', label: 'dense-only' },
            { id: 'r2', label: 'Run 4a01c9e7d2b3 on beir/fiqa', cells: [<Checkbox key="c" label="beir/fiqa" checked={false} onChange={() => {}} tabIndex={-1} disabled disabledReason="Other benchmark" />, '4a01c9e7d2b3', <StatusChip key="s" state="cancelled" />] },
          ]}
        />
      </Block>

      <Block name="Sheet">
        <Sheet>
          <Section heading="nDCG@10 at the output of each node" caption="read from the traces, not from a separate run" level={3}>
            <p>Charts are sections of one sheet, never a card each.</p>
          </Section>
          <Section heading="Latency per node" level={3}>
            <p>Not around a metric, not around rows, not to group fields, not to make something stand out.</p>
          </Section>
        </Sheet>
      </Block>

      <Block name="Inspector">
        <div className="rg-preview__row">
          <Inspector family="reranker" title="rerank" impl="reranker/onnx" onClose={() => {}} footer={<Button>Run up to here</Button>}>
            <p>Docked beside the canvas.</p>
          </Inspector>
        </div>
        <div className="rg-preview__canvas">
          <Inspector family="retriever" title="bm25" impl="retriever/bm25" floating onClose={() => {}}>
            <p>Floating over the canvas.</p>
          </Inspector>
        </div>
      </Block>

      <Block name="Toast">
        <div>
          <Toast action={{ label: 'Undo', onClick: () => {} }}>
            Deleted node <b>rerank</b> and its 2 edges.
          </Toast>
        </div>
        <div>
          <Toast tone="critical" action={{ label: 'Open Setup', onClick: () => {} }}>
            Launch refused: generator <b>qwen2.5-7b</b> is unreachable.
          </Toast>
        </div>
      </Block>

      <Block name="InlineMessage">
        <InlineMessage tone="critical" title="This run failed at node rerank on 300 of 300 queries." action={<Button size="s">Open in Replay</Button>}>
          The model file needs opset 18; this build reads up to 17. The trace up to fusion is kept.
        </InlineMessage>
        <InlineMessage tone="warning" title="beir/fiqa is downloading and not verified yet.">
          Runs on it wait until its sha256 digest matches the manifest.
        </InlineMessage>
        <InlineMessage tone="info" title="Adding a judge changes the run's hash.">
          A judged run is a different run, on purpose.
        </InlineMessage>
      </Block>

      <Block name="Progress">
        <Progress state="queued" value={0} total={10570} label="Queued, 2 runs ahead" detail="starts in about 3 min" />
        <Progress state="running" value={3982} total={10570} label="3,982 / 10,570 questions" detail="38%, about 6 min left" />
        <Progress state="done" value={300} total={300} label="Done in 9 min 12 s. nDCG@10 0.7217, +0.073 on dense-only" action={<Button size="s">Compare</Button>} />
        <Progress state="failed" value={1} total={300} label="Failed at rerank, query 1 of 300" action={<Button size="s">Open in Replay</Button>} />
      </Block>

      <Block name="EmptyState">
        <Sheet>
          <EmptyState
            heading="No runs in this workspace yet"
            action={
              <Button kind="primary" size="l" icon="play">
                Run the starter pipeline
              </Button>
            }
            secondary={<Button size="l">Open the Editor</Button>}
          >
            The starter pipeline fuses bm25 and dense retrieval with rrf. It needs no service.
          </EmptyState>
        </Sheet>
      </Block>

      <Block name="RankStrip">
        <div className="rg-preview__row">
          <RankStrip hits={[]} />
          <RankStrip hits={[1, 2]} />
          <RankStrip hits={[3, 7, 9]} />
          <RankStrip hits={[2]} cut={5} />
          <RankStrip hits={[1, 4]} large />
        </div>
      </Block>

      <Block name="SegmentedControl">
        <SegmentedControl
          label="Replay mode"
          value={s.mode}
          onChange={s.setMode}
          options={[
            { value: 'single', label: 'Single' },
            { value: 'side', label: 'Side by side' },
            { value: 'diff', label: 'Diff', disabled: true, reason: 'Pick two runs to diff' },
          ]}
        />
      </Block>

      <Block name="Tabs">
        <Tabs
          label="Setup sections"
          selected={s.tab}
          onSelect={s.setTab}
          tabs={[
            { id: 'workspace', label: 'Workspace' },
            { id: 'benchmarks', label: 'Benchmarks', count: 3 },
            { id: 'services', label: 'Services', count: 2 },
            { id: 'build', label: 'This build' },
          ]}
        />
      </Block>

      <Block name="TopBar">
        <TopBar
          workspace="~/ragondin-ws"
          links={['Runs', 'Pipeline', 'Compare', 'Replay', 'Editor', 'Setup'].map((label) => ({ label, href: `#${label.toLowerCase()}-${theme}`, current: label === 'Compare' }))}
          services={[
            { name: 'qwen2.5-7b', connected: true },
            { name: 'bge-embedder', connected: false },
          ]}
          status={{ label: 'disconnected — retrying', connected: false }}
          end={
            <Button kind="quiet" size="s" icon="sun">
              Theme
            </Button>
          }
        />
      </Block>

      <Block name="StatusDot">
        <div className="rg-preview__row">
          <StatusDot connected />
          <StatusDot connected={false} />
        </div>
      </Block>

      <Block name="NodeCard">
        <div className="rg-preview__cards">
          {CARDS.map(([caption, props]) => (
            <figure key={caption}>
              <Caption>{caption}</Caption>
              <NodeCard {...props} />
            </figure>
          ))}
        </div>
      </Block>

      <Block name="Canvas">
        <div className="rg-preview__flow">
          <Canvas graph={HYBRID_RERANK_GEN} label={`hybrid-rerank-gen, ${theme}`} />
        </div>
        <div className="rg-preview__flow">
          <Canvas graph={HYBRID_RERANK_GEN} label={`hybrid-rerank-gen replayed, ${theme}`} overlay={REPLAY} />
        </div>
      </Block>
    </div>
  );
}

/**
 * The reviewer's tool: every primitive in every state, in both themes side by
 * side. Dev server only; never part of the production bundle.
 */
export function Preview() {
  return (
    <div className="rg-preview">
      <Column theme="light" />
      <Column theme="dark" />
    </div>
  );
}
