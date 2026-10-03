import { API_BASE } from './base.ts';

/**
 * What a screen shows of an event stream. `disconnected` always means
 * "retrying": the wrapper never gives up, and never shows a stream as current
 * while it is not (the front-end design, § 8).
 */
export type ConnectionState = 'connecting' | 'connected' | 'disconnected';

export type EventHandlers = {
  onState?: (state: ConnectionState) => void;
  onMessage?: (data: string) => void;
  /**
   * Named events — the SSE `event` field — to listen for, each handed on
   * with its name. `onMessage` receives only unnamed ones, as `EventSource`
   * dispatches them.
   */
  events?: { names: readonly string[]; on: (name: string, data: string) => void };
  /**
   * Called on every connection after the first: the server may have been
   * restarted as another build in between, so the caller re-checks the build
   * identity (ADR-C36 § 1). The first connection is not re-checked here; the
   * page load already did.
   */
  onReconnect?: () => void;
};

export type EventStream = {
  close(): void;
  /**
   * Drops the connection and opens a new one after the retry delay. A new
   * `EventSource` carries no `Last-Event-ID`, so the server begins it afresh:
   * the remedy for a stream whose last event could not be read.
   */
  restart(): void;
};

const FIRST_RETRY_MS = 1000;
const LONGEST_RETRY_MS = 10_000;

/**
 * An event stream under the API's base address. The browser retries a
 * dropped stream by itself while it can; when it gives up — the source is
 * closed, as after an error status — this wrapper opens a new one, waiting
 * twice as long after each failure, up to ten seconds.
 */
export function openEvents(path: string, handlers: EventHandlers): EventStream {
  let source: EventSource | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let opened = false;
  let delay = FIRST_RETRY_MS;
  let closed = false;

  const connect = () => {
    const current = new EventSource(`${API_BASE}${path}`);
    source = current;
    current.onopen = () => {
      delay = FIRST_RETRY_MS;
      handlers.onState?.('connected');
      if (opened) handlers.onReconnect?.();
      opened = true;
    };
    current.onmessage = (event: MessageEvent<string>) => handlers.onMessage?.(event.data);
    for (const name of handlers.events?.names ?? []) {
      current.addEventListener(name, (event: MessageEvent<string>) => handlers.events?.on(name, event.data));
    }
    current.onerror = () => {
      handlers.onState?.('disconnected');
      if (current.readyState !== EventSource.CLOSED || closed) return;
      retry(current);
    };
  };

  // The source is given up on: closed, and a new one opened after the delay.
  const retry = (current: EventSource) => {
    current.close();
    timer = setTimeout(() => {
      timer = null;
      connect();
    }, delay);
    delay = Math.min(delay * 2, LONGEST_RETRY_MS);
  };

  handlers.onState?.('connecting');
  connect();
  return {
    close: () => {
      closed = true;
      if (timer !== null) clearTimeout(timer);
      source?.close();
    },
    restart: () => {
      if (closed || timer !== null || source === null) return;
      handlers.onState?.('disconnected');
      retry(source);
    },
  };
}
