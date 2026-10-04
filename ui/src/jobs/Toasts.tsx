// A run's outcome as a toast (the front-end design, § 8): "Run done — open",
// "Run failed at rerank — open", "Run cancelled". Raised once per job, on the
// transition the page saw — an event, or a resync after the stream was down —
// from a state not ended to an ended one; what had already ended when the
// page opened raises nothing. ARCHITECTURE.md § The job stream.
import { useLayoutEffect, useRef, useState } from 'react';
import { Toast } from '../../design/index.ts';
import type { Jobs } from '../api/jobs.ts';
import type { JobSummary } from '../api/types.ts';
import { navigate, type Route } from '../routes.ts';
import { useJobEvents } from './queue.tsx';
import './Toasts.css';

/** One outcome to say. */
export type Outcome = { job: string; tone: 'good' | 'critical'; words: string; detail: string; open: Route | null; persist: boolean };

const ENDED = new Set(['done', 'failed', 'cancelled']);

/** The outcome a run job's ended state says; null for a job that is not a run or has not ended. */
export function outcomeOf(job: JobSummary): Outcome | null {
  const { work, state } = job;
  if (work.kind !== 'run') return null;
  const detail = `${work.pipeline} on ${work.benchmark}`;
  switch (state.kind) {
    case 'done':
      return { job: job.id, tone: 'good', words: 'Run done', detail, open: state.run_id === null ? null : { screen: 'replay', run: state.run_id }, persist: false };
    case 'failed':
      // A failure needs action, so it stays until acted on.
      return { job: job.id, tone: 'critical', words: state.at_node === null ? 'Run failed' : `Run failed at ${state.at_node}`, detail, open: { screen: 'runs', job: job.id }, persist: true };
    case 'cancelled':
      return { job: job.id, tone: 'good', words: 'Run cancelled', detail, open: null, persist: false };
    default:
      return null;
  }
}

/** The run jobs `after` has ended that `before` knew as not ended, in the queue's order. */
export function endedBetween(before: Jobs, after: Jobs): Outcome[] {
  return [...after.values()].flatMap((job) => {
    const was = before.get(job.id);
    if (was === undefined || ENDED.has(was.state.kind) || !ENDED.has(job.state.kind)) return [];
    const outcome = outcomeOf(job);
    return outcome === null ? [] : [outcome];
  });
}

/**
 * The toasts, in a region fixed over the page's corner, so one arriving moves
 * nothing on the screen. Each toast is its own live region — a failure an
 * alert, the rest a status — and no toast takes focus as it appears.
 */
export function JobToasts() {
  const [shown, setShown] = useState<Outcome[]>([]);
  const region = useRef<HTMLElement>(null);
  // Where focus goes once a toast that held it has left: the place it held.
  const refocus = useRef<number | null>(null);

  useJobEvents((before, after) => {
    // Once per job, whatever replays: a job the queue already held as ended is never said again.
    const fresh = endedBetween(before, after);
    if (fresh.length > 0) setShown((all) => [...all, ...fresh]);
  });

  const dismiss = (job: string) => {
    const at = shown.findIndex((o) => o.job === job);
    const hadFocus = region.current?.contains(document.activeElement) ?? false;
    if (hadFocus) refocus.current = at;
    setShown((all) => all.filter((o) => o.job !== job));
  };

  // Focus was in the toast that left: to the one now in its place, else the region itself.
  useLayoutEffect(() => {
    const at = refocus.current;
    if (at === null) return;
    refocus.current = null;
    const closes = region.current?.querySelectorAll<HTMLElement>('.rg-toast__close') ?? [];
    (closes[Math.min(at, closes.length - 1)] ?? region.current)?.focus();
  }, [shown]);

  return (
    <section ref={region} className="rg-toasts" aria-label="Notifications" tabIndex={-1}>
      {shown.map((o) => (
        <Toast
          key={o.job}
          tone={o.tone}
          persist={o.persist}
          onDismiss={() => dismiss(o.job)}
          {...(o.open === null
            ? {}
            : {
                action: {
                  label: 'Open',
                  onClick: () => {
                    dismiss(o.job);
                    navigate(o.open as Route);
                  },
                },
              })}
        >
          {o.words} <span className="rg-toasts__detail">· {o.detail}</span>
        </Toast>
      ))}
    </section>
  );
}
