import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openEvents, type ConnectionState } from './events.ts';
import { FakeEventSource, installFakeEventSource } from './testing.ts';

beforeEach(() => {
  installFakeEventSource();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const open = (onReconnect = vi.fn()) => {
  const states: ConnectionState[] = [];
  const messages: string[] = [];
  const stream = openEvents('/jobs/events', {
    onState: (s) => states.push(s),
    onMessage: (data) => messages.push(data),
    onReconnect,
  });
  return { stream, states, messages, onReconnect };
};

describe('openEvents', () => {
  it('opens the stream on the relative base address, connecting until it opens', () => {
    const { states } = open();
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.latest().url).toBe('/api/v1/jobs/events');
    expect(states).toEqual(['connecting']);
    FakeEventSource.latest().open();
    expect(states).toEqual(['connecting', 'connected']);
  });

  it('passes each message on', () => {
    const { messages } = open();
    FakeEventSource.latest().open();
    FakeEventSource.latest().emit('{"state":"queued"}');
    expect(messages).toEqual(['{"state":"queued"}']);
  });

  it('shows disconnected while the browser retries a dropped stream, and re-checks on reopening', () => {
    const { states, onReconnect } = open();
    const source = FakeEventSource.latest();
    source.open();
    source.fail();
    expect(states.at(-1)).toBe('disconnected');
    expect(onReconnect).not.toHaveBeenCalled();
    source.open();
    expect(states.at(-1)).toBe('connected');
    expect(onReconnect).toHaveBeenCalledTimes(1);
  });

  it('reconnects a closed stream itself, after a delay, and re-checks on the new connection', () => {
    const { states, onReconnect } = open();
    FakeEventSource.latest().open();
    FakeEventSource.latest().drop();
    expect(states.at(-1)).toBe('disconnected');
    expect(FakeEventSource.instances).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[0]?.closed).toBe(true);
    expect(states.at(-1)).toBe('disconnected');
    FakeEventSource.latest().open();
    expect(states.at(-1)).toBe('connected');
    expect(onReconnect).toHaveBeenCalledTimes(1);
  });

  it('keeps retrying, further apart, while the stream stays closed', () => {
    open();
    FakeEventSource.latest().drop();
    vi.advanceTimersByTime(1000);
    FakeEventSource.latest().drop();
    vi.advanceTimersByTime(1000);
    expect(FakeEventSource.instances).toHaveLength(2);
    vi.advanceTimersByTime(1000);
    expect(FakeEventSource.instances).toHaveLength(3);
  });

  it('does not re-check the first connection, which the page load already checked', () => {
    const { onReconnect } = open();
    FakeEventSource.latest().open();
    expect(onReconnect).not.toHaveBeenCalled();
  });

  it('closes the stream and any pending retry', () => {
    const { stream } = open();
    FakeEventSource.latest().drop();
    stream.close();
    vi.advanceTimersByTime(60_000);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.latest().closed).toBe(true);
  });
});
