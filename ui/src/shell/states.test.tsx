/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ApiProblem } from '../api/client.ts';
import { ErrorState, Loading, Resource, type RequestState } from './states.tsx';

const PROBLEM: ApiProblem = {
  code: 'run_not_found',
  message: 'No run 1234 in this workspace.',
  hint: 'Open Runs to see the runs this workspace holds.',
  location: null,
  status: 404,
};

describe('Loading', () => {
  it('says what is in flight, in words, as a live status', () => {
    render(<Loading label="Reading the workspace" />);
    const status = screen.getByRole('status');
    expect(status.textContent).toBe('Reading the workspace');
    expect(status.getAttribute('aria-busy')).toBe('true');
  });
});

describe('ErrorState', () => {
  it('renders an ApiProblem inline: its message, its hint and its code', () => {
    render(<ErrorState problem={PROBLEM} />);
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toContain('No run 1234 in this workspace.');
    expect(alert.textContent).toContain('Open Runs to see the runs this workspace holds.');
    expect(screen.getByText('run_not_found').tagName).toBe('CODE');
  });

  it('names the node a validation failure is located at', () => {
    render(<ErrorState problem={{ ...PROBLEM, code: 'pipeline_invalid', location: { node: 'rerank', edge: null } }} />);
    expect(screen.getByRole('alert').textContent).toContain('rerank');
  });

  it('offers the retry it is given', () => {
    const onRetry = vi.fn();
    render(<ErrorState problem={PROBLEM} onRetry={onRetry} />);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});

describe('Resource', () => {
  const view = (state: RequestState<string>) =>
    render(
      <Resource state={state} loading="Reading the run">
        {(value) => <p>Loaded {value}</p>}
      </Resource>,
    );

  it('shows the labelled loading state while the request is in flight', () => {
    view({ status: 'loading' });
    expect(screen.getByRole('status').textContent).toBe('Reading the run');
  });

  it('shows the error state with the problem when the request failed', () => {
    view({ status: 'error', problem: PROBLEM });
    expect(screen.getByRole('alert').textContent).toContain('No run 1234');
  });

  it('shows the loaded state with the value', () => {
    view({ status: 'loaded', value: 'run 1234' });
    expect(screen.getByText('Loaded run 1234')).toBeTruthy();
  });
});
