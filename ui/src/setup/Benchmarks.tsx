// The Benchmarks section: one row per registry entry, in the listing's order,
// with its state, what that state means in digests, its ground truth and its
// licence; and the import of a local corpus. A benchmark is a pinned snapshot
// (docs/system-architecture.md § 9.1): there is no address field here.
import type { ReactNode, Ref } from 'react';
import { Button, Section, StatusChip, Table, type Status, type TableRow } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry } from '../api/types.ts';
import { ErrorState, Resource, type RequestState } from '../shell/states.tsx';
import { ImportForm } from './forms.tsx';
import { formatSize, groundTruthLabel, shortDigest } from './model.ts';

/** Why Download is refused in this build: no route downloads a benchmark until the job queue brings one. */
export const DOWNLOAD_REFUSED = 'Downloads arrive with the job queue.';

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

/** What a state means, in digests and words. */
function detail(entry: BenchmarkEntry): ReactNode {
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
      return <>{formatSize(state.size_bytes)} to download</>;
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

/** Download, refused with its reason until a route downloads a benchmark. */
export function DownloadButton() {
  return (
    <Button size="s" icon="download" disabled disabledReason={DOWNLOAD_REFUSED}>
      Download
    </Button>
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

function rows(benchmarks: readonly BenchmarkEntry[]): TableRow[] {
  return benchmarks.map((b) => ({
    id: b.name,
    cells: [
      <span className="rg-setup__name">{b.name}</span>,
      <StatusChip state={CHIP[b.state.kind]}>{b.state.kind}</StatusChip>,
      <span className="rg-setup__detail">{detail(b)}</span>,
      b.ground_truth === null ? null : groundTruthLabel(b.ground_truth),
      <Licence entry={b} />,
      b.state.kind === 'available' ? <DownloadButton /> : null,
    ],
  }));
}

export type BenchmarksProps = {
  state: RequestState<readonly BenchmarkEntry[]>;
  onRetry: () => void;
  onImport: (path: string, name: string) => Promise<ApiProblem | null>;
  anchor: Ref<HTMLElement>;
};

export function Benchmarks({ state, onRetry, onImport, anchor }: BenchmarksProps) {
  return (
    <Section heading="Benchmarks" caption="Pinned snapshots: a benchmark is ready when the dataset on disk digests to the version the manifest pins." anchor={anchor}>
      <Resource state={state} loading="Reading benchmarks" error={(problem) => <ErrorState problem={problem} onRetry={onRetry} />}>
        {(benchmarks) =>
          benchmarks.length === 0 ? (
            <p className="rg-setup__note">The registry lists no benchmark: this build’s manifest pins none, and nothing was imported.</p>
          ) : (
            <>
              <Table caption="Benchmarks the registry knows" columns={COLUMNS} rows={rows(benchmarks)} />
              {benchmarks.some((b) => b.state.kind === 'available') ? <p className="rg-setup__note">Download is refused for now: {DOWNLOAD_REFUSED.toLowerCase()}</p> : null}
            </>
          )
        }
      </Resource>
      <h3 className="rg-setup__subheading">Import a local corpus</h3>
      <ImportForm onImport={onImport} />
      <p className="rg-setup__note">Custom benchmarks with generated questions arrive with M8.</p>
    </Section>
  );
}
