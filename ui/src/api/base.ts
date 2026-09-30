/**
 * The one address the UI talks to: the JSON API of the binary that served the
 * page (ADR-C36 § 1). Relative on purpose, so that every request built from it
 * stays on the page's own origin; `src/api/` is the only module allowed to
 * touch the network (ARCHITECTURE.md § The network lint).
 */
export const API_BASE = '/api/v1';
