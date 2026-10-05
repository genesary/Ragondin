// `npm run dev:fixture`: the dev server against the fixture workspace, served
// by the real binary — one truth for the page under development, the Rust
// tests and the end-to-end journeys (ARCHITECTURE.md § The end-to-end
// journeys).
//
// RAGONDIN_E2E_BINARY names a binary built with `ui,bm25,onnx`, and
// RAGONDIN_E2E_FIXTURE the directory `just fixture-workspace` wrote; `just
// ui-dev-fixture` sets both. The binary serves the workspace itself, from it,
// on a port the system chooses; the dev server forwards `/api` to it
// (scripts/dev-proxy.mjs). A run launched from the page is filed in that
// workspace: regenerate it to start again.
import { spawn } from 'node:child_process';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

/** @param {string} name */
function required(name) {
  const value = process.env[name];
  if (value === undefined || value === '') {
    console.error(`${name} is not set: run \`just ui-dev-fixture\`, which builds the binary and the fixture workspace and names them.`);
    process.exit(1);
  }
  return value;
}

const workspace = join(required('RAGONDIN_E2E_FIXTURE'), 'workspace');
const binary = spawn(required('RAGONDIN_E2E_BINARY'), ['ui', '--workspace', workspace, '--port', '0'], {
  cwd: workspace,
  stdio: ['ignore', 'pipe', 'inherit'],
});
binary.on('exit', (code) => {
  console.error(`ragondin ui exited with ${code}`);
  process.exit(code ?? 1);
});

createInterface({ input: /** @type {import('node:stream').Readable} */ (binary.stdout) }).once('line', (line) => {
  const api = /http:\/\/\S+/.exec(line)?.[0];
  if (api === undefined) {
    console.error(`ragondin ui printed no address: ${line}`);
    binary.kill();
    process.exit(1);
  }
  console.log(line);
  // Arguments after `--` go to Vite: `npm run dev:fixture -- --port 9561`.
  const vite = spawn('npx', ['vite', ...process.argv.slice(2)], { stdio: 'inherit', env: { ...process.env, RAGONDIN_API: api } });
  const stop = () => {
    vite.kill();
    binary.kill();
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  vite.on('exit', (code) => {
    binary.kill();
    process.exit(code ?? 0);
  });
});
