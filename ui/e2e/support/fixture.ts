// The real binary over a workspace of the test's own, and the page pointed at
// it. ARCHITECTURE.md § The end-to-end journeys.
//
// `just test-ui-e2e` builds the binary with `ui,bm25,onnx` and writes the
// fixture workspace (bin/ragondin/tests/support/workspace.rs), and names both:
// RAGONDIN_E2E_BINARY, the binary; RAGONDIN_E2E_FIXTURE, the directory holding
// `fixture.json`, `workspace/` and `corpus/`. Each test copies `workspace/`
// (or starts from an empty directory), runs `ragondin ui --port 0` from it —
// the documents' model paths are relative to the workspace — and reads the
// address from the line it prints. Nothing is shared between tests, so a
// journey that launches a run changes no other test's workspace.
//
// No network: every request the page makes to another origin than the
// server's is aborted and recorded, and a test that made one fails.
import { spawn, type ChildProcess } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { test as base, expect } from '@playwright/test';

/** What `fixture.json` records: where things are, and each run's id. */
export type Fixture = {
  workspace: string;
  corpus: string;
  benchmark: string;
  benchmark_name: string;
  changed_top_k: number;
  runs: { dense_only: string; hybrid_rerank: string; unrecorded: string; changed: string };
};

/** A running `ragondin ui`. */
export type Ragondin = {
  /** The address it printed, `http://127.0.0.1:<port>/`. */
  url: string;
  /** The workspace it serves: a copy, or a new empty directory. */
  workspace: string;
};

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value === '') {
    throw new Error(`${name} is not set: run the journeys with \`just test-ui-e2e\`, which builds the binary and the fixture workspace and names them.`);
  }
  return value;
}

export function readFixture(): Fixture {
  return JSON.parse(readFileSync(join(required('RAGONDIN_E2E_FIXTURE'), 'fixture.json'), 'utf8')) as Fixture;
}

/** Starts the binary over `workspace` and resolves with the address it prints. */
function start(workspace: string): Promise<{ child: ChildProcess; url: string }> {
  const child = spawn(required('RAGONDIN_E2E_BINARY'), ['ui', '--workspace', workspace, '--port', '0'], {
    cwd: workspace,
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  return new Promise((resolve, reject) => {
    const lines = createInterface({ input: child.stdout! });
    child.once('error', reject);
    child.once('exit', (code) => reject(new Error(`ragondin ui exited with ${code} before printing its address`)));
    lines.once('line', (line) => {
      const url = /http:\/\/\S+/.exec(line)?.[0];
      if (url === undefined) reject(new Error(`ragondin ui printed no address: ${line}`));
      else resolve({ child, url: url.endsWith('/') ? url : `${url}/` });
    });
  });
}

type Options = { workspaceKind: 'fixture' | 'empty' };
type Fixtures = { fixture: Fixture; ragondin: Ragondin; oneOrigin: void };

export const test = base.extend<Options & Fixtures>({
  workspaceKind: ['fixture', { option: true }],
  // eslint-disable-next-line no-empty-pattern
  fixture: async ({}, use) => {
    await use(readFixture());
  },
  ragondin: async ({ fixture, workspaceKind }, use) => {
    const root = mkdtempSync(join(tmpdir(), 'ragondin-e2e-'));
    const workspace = join(root, 'workspace');
    if (workspaceKind === 'fixture') cpSync(fixture.workspace, workspace, { recursive: true });
    else mkdirSync(workspace);
    const { child, url } = await start(workspace);
    try {
      await use({ url, workspace });
    } finally {
      child.kill();
      rmSync(root, { recursive: true, force: true });
    }
  },
  baseURL: async ({ ragondin }, use) => {
    await use(ragondin.url);
  },
  oneOrigin: [
    async ({ context, ragondin }, use) => {
      const origin = new URL(ragondin.url).origin;
      const refused: string[] = [];
      await context.route('**/*', async (route) => {
        const url = route.request().url();
        if (new URL(url).origin === origin) await route.continue();
        else {
          refused.push(url);
          await route.abort('blockedbyclient');
        }
      });
      await use();
      expect(refused, 'the page reached only the origin that served it').toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };
