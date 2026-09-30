import './StatusDot.css';

/**
 * Whether something answers: a filled dot when it does, a hollow ring when it
 * does not. Always beside a word that says the same — the dot is hidden from
 * assistive technology, and colour is never its only carrier.
 */
export function StatusDot({ connected }: { connected: boolean }) {
  return <span className="rg-dot" data-connected={connected} aria-hidden="true" />;
}
