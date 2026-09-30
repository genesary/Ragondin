import type { ConnectionState } from '../api/events.ts';

const WORDS: Record<ConnectionState, string> = {
  connecting: 'connecting',
  connected: 'connected',
  disconnected: 'disconnected — retrying',
};

/**
 * The event stream's state, drawn as the top bar draws a service: a filled
 * dot and a word when connected, a hollow ring and the word otherwise. It
 * never reads as current while the stream is down (the front-end design, § 8).
 */
export function ConnectionStatus({ state }: { state: ConnectionState }) {
  return (
    <span className="rg-service" data-connected={state === 'connected'} role="status">
      <span className="rg-dot" aria-hidden="true" />
      {WORDS[state]}
    </span>
  );
}
