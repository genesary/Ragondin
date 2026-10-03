// Live validation: the document as it stands, sent to `POST /pipelines/validate`
// after every change, debounced, the request a newer document supersedes
// cancelled. Validity is what the server said about the document it was sent:
// nothing here judges one. ARCHITECTURE.md § The editor.
import { useEffect, useState } from 'react';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import { validationRequest, type WireDocument } from './document.ts';

/** How long the document must rest before it is sent. */
export const VALIDATE_DEBOUNCE_MS = 250;

export type Verdict =
  /** Sent, or about to be, and not answered yet. */
  | { status: 'checking' }
  /** The server's canonical hash: the identity a run of this document would carry (INV-8). */
  | { status: 'valid'; hash: string }
  /** `pipeline_invalid`, located when the server could locate it. */
  | { status: 'invalid'; problem: ApiProblem }
  /** No verdict: the request itself failed. */
  | { status: 'failed'; problem: ApiProblem };

/** The server's verdict on `doc`, for the last document asked about only. */
export function useValidation(client: ApiClient, doc: WireDocument): Verdict {
  const [verdict, setVerdict] = useState<{ of: WireDocument; verdict: Verdict } | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    const timer = setTimeout(() => {
      void client.post('/pipelines/validate', validationRequest(doc), { signal: abort.signal }).then((result) => {
        if (abort.signal.aborted) return;
        const answered: Verdict = result.ok
          ? { status: 'valid', hash: result.value.hash }
          : result.problem.code === 'pipeline_invalid'
            ? { status: 'invalid', problem: result.problem }
            : { status: 'failed', problem: result.problem };
        setVerdict({ of: doc, verdict: answered });
      });
    }, VALIDATE_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      abort.abort();
    };
  }, [client, doc]);
  // A verdict on another document is no verdict on this one.
  return verdict !== null && verdict.of === doc ? verdict.verdict : { status: 'checking' };
}
