// The dev server's fixture mode: `vite.config.ts` hands `/api` to the real
// binary when RAGONDIN_API names it — `npm run dev:fixture` starts one over the
// fixture workspace and sets it. ARCHITECTURE.md § The end-to-end journeys.
//
// The page still reaches one origin, the dev server's, which forwards `/api`
// (ARCHITECTURE.md § The one-address rule). The binary answers only the Host
// it printed and refuses a state-changing request from another Origin
// (runtime/ragondin-api/ARCHITECTURE.md), so both are rewritten to its own:
// to the binary, the request comes from the page it serves. Loopback only, as
// the binary binds.

/**
 * The `server.proxy` entry for `api`, the binary's address, or undefined when
 * none is given.
 * @param {string | undefined} api
 * @returns {Record<string, { target: string, changeOrigin: boolean, headers: Record<string, string> }> | undefined}
 */
export function devProxy(api) {
  if (api === undefined || api === '') return undefined;
  const url = new URL(api);
  if (url.protocol !== 'http:') {
    throw new Error(`RAGONDIN_API must be the http://127.0.0.1:<port> address \`ragondin ui\` prints, not ${api}`);
  }
  if (url.hostname !== '127.0.0.1' && url.hostname !== '[::1]') {
    throw new Error(`RAGONDIN_API must be a loopback address, as \`ragondin ui\` binds: ${api}`);
  }
  const origin = url.origin;
  return { '/api': { target: origin, changeOrigin: true, headers: { origin } } };
}
