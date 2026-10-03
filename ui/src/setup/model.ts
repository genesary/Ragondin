// What the Setup screen reads off the API's answers, as pure functions:
// sizes and digests in words, the first-launch rule, the build identity's
// parts. ARCHITECTURE.md § The Setup screen.
import type { BenchmarkEntry, GroundTruth, ServiceStatus } from '../api/types.ts';

const UNITS = ['B', 'kB', 'MB', 'GB', 'TB'] as const;

/** A size in decimal units, as a download page states one: one decimal under ten, whole above. */
export function formatSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const shown = unit === 0 || value >= 100 ? Math.round(value).toString() : (Math.round(value * 10) / 10).toString();
  return `${shown} ${UNITS[unit]}`;
}

/** A digest shortened as the rest of the UI shortens a hash; the full value goes in its title. */
export const shortDigest = (digest: string) => digest.slice(0, 12);

const GROUND_TRUTH: Record<GroundTruth, string> = {
  qrels: 'qrels',
  reference_answers: 'reference answers',
  both: 'qrels and reference answers',
  none: 'no ground truth',
};

/** What a benchmark's ground truth lets a run measure, in words. */
export const groundTruthLabel = (truth: GroundTruth) => GROUND_TRUTH[truth];

/**
 * Whether the workspace is at its first launch: no benchmark on disk — an
 * `available` entry is the manifest's offer, not something held — and no
 * service bound. The screen is then an invitation rather than four sections.
 */
export function isFirstLaunch(benchmarks: readonly BenchmarkEntry[], services: readonly ServiceStatus[]): boolean {
  return services.length === 0 && benchmarks.every((b) => b.state.kind === 'available');
}

type Available = BenchmarkEntry & { state: { kind: 'available'; size_bytes: number } };

const isAvailable = (b: BenchmarkEntry): b is Available => b.state.kind === 'available';

/** The available benchmark with the fewest bytes — a good first run, read from the manifest's sizes — or null. */
export function smallestAvailable(benchmarks: readonly BenchmarkEntry[]): Available | null {
  return benchmarks.filter(isAvailable).reduce<Available | null>((best, b) => (best === null || b.state.size_bytes < best.state.size_bytes ? b : best), null);
}

/** The build identity's two parts, `<version>+<commit>` (ARCHITECTURE.md § The build identity handshake). */
export function splitBuild(build: string): { version: string; commit: string | null } {
  const at = build.indexOf('+');
  return at < 0 ? { version: build, commit: null } : { version: build.slice(0, at), commit: build.slice(at + 1) };
}

/** A binding's key, `<family>/<name>`, as `--remote` and `workspace.toml` spell it. */
export const serviceKey = (s: { family: string; name: string }) => `${s.family}/${s.name}`;
