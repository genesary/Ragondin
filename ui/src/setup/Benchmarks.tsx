// The Benchmarks section: one row per registry entry, in the listing's order,
// with its state, what that state means in digests, its ground truth and its
// licence; an available one's download, followed on the job stream; and the
// import of a local corpus. A benchmark is a pinned snapshot
// (docs/system-architecture.md § 9.1): there is no address field here.
import type { ReactNode, Ref } from 'react';
import { Button, Section, StatusChip, Table, type Status, type TableRow } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry } from '../api/types.ts';
import { STREAM_DOWN, STREAM_DOWN_LIVE } from '../jobs/stream.ts';
import { ErrorState, Resource, type RequestState } from '../shell/states.tsx';
import { ImportForm } from './forms.tsx';
import { formatSize, groundTruthLabel, shortDigest, type DownloadView } from './model.ts';

/** The downloads the screen follows: where each benchmark's stands, and how to ask for one. */
export type Downloads = {
  view: (name: string) => DownloadView;
  start: (name: string) => void;
  /** The job stream is down and retrying: what a row says of a live job is the last known, not the current. */
  streamDown: boolean;
};


/** Whether a download's state comes from a job still under way, which only the stream keeps current. */
export const isLive = (view: DownloadView) => view.kind === 'queued' || view.kind === 'running' || view.kind === 'verifying';

/** How full the meter is: the bytes over the job's total, or over the manifest's size before it gives one; empty when that is zero. */
export function fraction(done: number, total: number | null, size: number): number {
  const of = total ?? size;
  return of > 0 ? done / of : 0;
}

const CHIP: Record<BenchmarkEntry['state']['kind'], Exclude<Status, 'running'>> = {
  ready: 'done',
  local: 'done',
  available: 'queued',
  differs: 'warning',
  unreadable: 'failed',
};

/** A digest, short, with the full value on hover. */
export function Digest({ value }: { value: string }) {
  return (
    <code className="rg-setup__digest" title={value}>
      {shortDigest(value)}
    </code>
  );
}

/**
 * What a download says, in the row's words. A failure is the API's own words
 * in an alert, announced as it appears; progress is not a live region, so a
 * tick every hundredth of the snapshot is not read aloud.
 */
export function downloadWords(view: DownloadView, size: number, streamDown = false): ReactNode {
  const words = currentWords(view, size);
  return streamDown && isLive(view) ? <>{words} · last known</> : words;
}

function currentWords(view: DownloadView, size: number): ReactNode {
  switch (view.kind) {
    case 'idle':
      return <>{formatSize(size)} to download</>;
    case 'submitting':
      return <>Asking for the download…</>;
    case 'queued':
      return <>Queued, {formatSize(size)} to download</>;
    case 'running':
      // Before the first tick the job knows no total: the manifest's size is the snapshot's.
      return (
        <>
          Downloading… {formatSize(view.done)} of {formatSize(view.total ?? size)}
        </>
      );
    case 'verifying':
      return <>Downloaded; reading its digest…</>;
    case 'failed':
      return <span role="alert">Download failed: {view.error}</span>;
    case 'refused':
      return (
        <span role="alert">
          {view.problem.message} {view.problem.hint} <code>{view.problem.code}</code>
        </span>
      );
    case 'cancelled':
      return <>Download cancelled.</>;
  }
}

/** The state chip of an available benchmark, as its download moves it. */
function downloadChip(view: DownloadView, size: number): ReactNode {
  switch (view.kind) {
    case 'idle':
      return <StatusChip state={CHIP.available}>available</StatusChip>;
    case 'submitting':
    case 'queued':
      return <StatusChip state="queued">queued</StatusChip>;
    case 'running':
      return (
        <StatusChip state="running" fraction={fraction(view.done, view.total, size)}>
          downloading
        </StatusChip>
      );
    case 'verifying':
      return (
        <StatusChip state="running" fraction={1}>
          verifying
        </StatusChip>
      );
    case 'failed':
    case 'refused':
      return <StatusChip state="failed">failed</StatusChip>;
    case 'cancelled':
      return <StatusChip state="cancelled">cancelled</StatusChip>;
  }
}

/** What a state means, in digests and words. */
function detail(entry: BenchmarkEntry, downloads: Downloads): ReactNode {
  const state = entry.state;
  switch (state.kind) {
    case 'ready':
      return (
        <>
          verified <Digest value={state.dataset_version} />
        </>
      );
    case 'local':
      return (
        <>
          imported, verified <Digest value={state.dataset_version} />
        </>
      );
    case 'available':
      return downloadWords(downloads.view(entry.name), state.size_bytes, downloads.streamDown);
    case 'differs':
      return (
        <>
          expected <Digest value={state.expected} />, found <Digest value={state.found} />: the dataset on disk is not the one {/* An import carries no licence: its digest is the one recorded at import, not a manifest's. */}
          {entry.licence === null ? 'recorded at import' : 'the manifest pins'}
        </>
      );
    case 'unreadable':
      return <>{state.error}</>;
  }
}

/** The licence as the manifest records it, a link when it gives where the terms are stated. */
export function Licence({ entry }: { entry: BenchmarkEntry }) {
  if (entry.licence === null) return null;
  return entry.licence_url === null ? (
    <>{entry.licence}</>
  ) : (
    <a href={entry.licence_url} rel="noreferrer">
      {entry.licence}
    </a>
  );
}

/**
 * A benchmark's one download control: Download, then Downloading while its
 * job is under way, then Retry when it was refused, failed or cancelled. One
 * button whose label changes, so focus stays on it throughout.
 */
export function DownloadButton({ view, onStart }: { view: DownloadView; onStart: () => void }) {
  switch (view.kind) {
    case 'idle':
    case 'submitting':
      return (
        <Button size="s" icon="download" busy={view.kind === 'submitting'} busyLabel="Downloading" onClick={onStart}>
          Download
        </Button>
      );
    case 'queued':
    case 'running':
    case 'verifying':
      return (
        <Button size="s" icon="download" disabled disabledReason="The download is under way; its progress is beside it.">
          Downloading
        </Button>
      );
    case 'failed':
    case 'refused':
    case 'cancelled':
      return (
        <Button size="s" icon="download" onClick={onStart}>
          Retry
        </Button>
      );
  }
}

/** The action cell of every row, one control high whether it holds a button or not, so a button coming or going moves nothing. */
export function ActionSlot({ benchmark, children }: { benchmark: string; children?: ReactNode }) {
  return (
    <span className="rg-setup__action" data-benchmark={benchmark}>
      {children}
    </span>
  );
}

const COLUMNS = [
  { id: 'name', label: 'Benchmark' },
  { id: 'state', label: 'State' },
  { id: 'detail', label: 'Digest or size' },
  { id: 'truth', label: 'Ground truth' },
  { id: 'licence', label: 'Licence' },
  { id: 'action', label: <span className="rg-visually-hidden">Action</span> },
];

function rows(benchmarks: readonly BenchmarkEntry[], downloads: Downloads): TableRow[] {
  return benchmarks.map((b) => {
    const view = b.state.kind === 'available' ? downloads.view(b.name) : null;
    return {
      id: b.name,
      cells: [
        <span className="rg-setup__name">{b.name}</span>,
        b.state.kind === 'available' && view !== null ? downloadChip(view, b.state.size_bytes) : <StatusChip state={CHIP[b.state.kind]}>{b.state.kind}</StatusChip>,
        <span className="rg-setup__detail">{detail(b, downloads)}</span>,
        b.ground_truth === null ? null : groundTruthLabel(b.ground_truth),
        <Licence entry={b} />,
        <ActionSlot benchmark={b.name}>{view === null ? null : <DownloadButton view={view} onStart={() => downloads.start(b.name)} />}</ActionSlot>,
      ],
    };
  });
}

export type BenchmarksProps = {
  state: RequestState<readonly BenchmarkEntry[]>;
  /** A read of the listing in place that failed: the table stays, and this says why it is not newer. */
  stale: ApiProblem | null;
  onRetry: () => void;
  /** Reads the listing again in place, for `stale`'s Retry. */
  onRefresh: () => void;
  onImport: (path: string, name: string) => Promise<ApiProblem | null>;
  downloads: Downloads;
  anchor: Ref<HTMLElement>;
};

export function Benchmarks({ state, stale, onRetry, onRefresh, onImport, downloads, anchor }: BenchmarksProps) {
  return (
    <Section heading="Benchmarks" caption="Pinned snapshots: a benchmark is ready when the dataset on disk digests to the version the manifest pins." anchor={anchor}>
      <Resource state={state} loading="Reading benchmarks" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(benchmarks) =>
          benchmarks.length === 0 ? (
            <p className="rg-setup__note">The registry lists no benchmark: this build’s manifest pins none, and nothing was imported.</p>
          ) : (
            <>
              <Table caption="Benchmarks the registry knows" columns={COLUMNS} rows={rows(benchmarks, downloads)} />
              {stale === null ? null : <ErrorState problem={stale} onRetry={onRefresh} />}
            </>
          )
        }
      </Resource>
      {/* Always present, as high as its longer sentence, so saying the stream is down moves nothing. */}
      <p className="rg-setup__said rg-setup__stream" role="status">
        {!downloads.streamDown ? null : state.status === 'loaded' && state.value.some((b) => b.state.kind === 'available' && isLive(downloads.view(b.name))) ? STREAM_DOWN_LIVE : STREAM_DOWN}
      </p>
      <h3 className="rg-setup__subheading">Import a local corpus</h3>
      <ImportForm onImport={onImport} />
      <p className="rg-setup__note">Custom benchmarks with generated questions arrive with M8.</p>
    </Section>
  );
}
