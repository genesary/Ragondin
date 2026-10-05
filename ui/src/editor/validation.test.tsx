/** @vitest-environment happy-dom */
import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi } from '../api/testing.ts';
import { emptyDocument, type WireDocument } from './document.ts';
import { useValidation, VALIDATE_DEBOUNCE_MS } from './validation.ts';

const HASH = 'd'.repeat(64);
const withNode = (id: string): WireDocument => ({ pipeline: { inputs: ['query'], nodes: [{ id, component: 'retriever', impl: 'bm25', inputs: ['query'], params: {} }] } });
const sent = (api: ReturnType<typeof mockApi>) => api.requests.flatMap((r, i) => (r === 'POST /api/v1/pipelines/validate' ? [i] : []));

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('live validation', () => {
  it('waits for the document to rest: three changes inside the debounce send one request, of the last', async () => {
    vi.useFakeTimers();
    const api = mockApi({ 'POST /pipelines/validate': { body: { hash: HASH, rendering: null } } });
    const client = createApiClient();
    const { rerender } = renderHook(({ doc }) => useValidation(client, doc), { initialProps: { doc: emptyDocument() } });
    rerender({ doc: withNode('a') });
    rerender({ doc: withNode('b') });
    act(() => vi.advanceTimersByTime(VALIDATE_DEBOUNCE_MS - 1));
    expect(sent(api)).toHaveLength(0);
    await act(async () => vi.advanceTimersByTime(1));
    expect(sent(api)).toHaveLength(1);
    expect(api.bodies[sent(api)[0]!]).toEqual({ typed: withNode('b') });
  });

  it('cancels the request a newer document supersedes, and shows only the newer verdict', async () => {
    let first: (v: { body: { hash: string; rendering: string | null } }) => void = () => {};
    let calls = 0;
    const api = mockApi({
      'POST /pipelines/validate': () => {
        calls += 1;
        return calls === 1 ? new Promise((resolve) => (first = resolve)) : { body: { hash: HASH, rendering: null } };
      },
    });
    const client = createApiClient();
    const { result, rerender } = renderHook(({ doc }) => useValidation(client, doc), { initialProps: { doc: withNode('a') } });
    await waitFor(() => expect(sent(api)).toHaveLength(1));
    rerender({ doc: withNode('b') });
    expect(api.signals[sent(api)[0]!]?.aborted).toBe(true);
    await waitFor(() => expect(result.current).toEqual({ status: 'valid', hash: HASH, rendering: null }));
    await act(async () => first({ body: { hash: 'e'.repeat(64), rendering: null } }));
    expect(result.current).toEqual({ status: 'valid', hash: HASH, rendering: null });
  });
});
