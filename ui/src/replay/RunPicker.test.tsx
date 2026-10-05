/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi } from '../api/testing.ts';
import type { Problem } from '../api/types.ts';
import { DENSE, HYBRID, LISTING } from './fixtures.ts';
import { RunPicker } from './RunPicker.tsx';

const show = () => render(<RunPicker client={createApiClient()} />);

describe('Replay before a run is chosen', () => {
  it('offers the runs to replay, and opens the one chosen', async () => {
    mockApi({ 'GET /runs': { body: LISTING } });
    show();
    expect((await screen.findByRole('heading', { name: 'Choose a run to replay' })).tagName).toBe('H3');
    const run = screen.getByLabelText('Run') as HTMLSelectElement;
    expect([...run.options].map((o) => o.textContent)).toEqual([
      `hybrid-rerank-gen · ${HYBRID.slice(0, 12)} · beir/scifact`,
      `dense-only · ${DENSE.slice(0, 12)} · beir/scifact`,
      expect.stringMatching(/^hybrid-broken · /),
      expect.stringMatching(/^dense-nfcorpus · .* · beir\/nfcorpus$/),
    ]);
    const open = screen.getByRole('link', { name: 'Replay this run' });
    expect(open.getAttribute('href')).toBe(`#replay/${HYBRID}`);
    fireEvent.change(run, { target: { value: DENSE } });
    expect(open.getAttribute('href')).toBe(`#replay/${DENSE}`);
  });

  it('says there is no run yet, and leads to Runs, in an empty workspace', async () => {
    mockApi({ 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } } });
    show();
    expect(await screen.findByRole('heading', { name: 'No run to replay yet' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe('#runs');
  });

  it('says why, with Retry, when the runs cannot be listed', async () => {
    const problem: Problem = { type: 'urn:ragondin:problem:backend_failed', title: 'backend_failed', status: 500, detail: 'The store did not answer.', code: 'backend_failed', hint: 'Try again.' };
    mockApi({ 'GET /runs': { problem } });
    show();
    expect(await screen.findByText(/The store did not answer\./)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
  });
});
