// The canvas library stays behind src/canvas/: a screen imports the Canvas,
// never the library, so the canvas can change its wrapper without touching a
// screen. Tested against the real configuration. ARCHITECTURE.md § The canvas.
import { ESLint, type Linter } from 'eslint';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const UI = fileURLToPath(new URL('..', import.meta.url));
const eslint = new ESLint({ cwd: UI });

async function lint(code: string, file: string): Promise<Linter.LintMessage[]> {
  const [result] = await eslint.lintText(code, { filePath: join(UI, file) });
  if (result === undefined) throw new Error(`ESLint returned no result for ${file}`);
  return result.messages;
}

const IMPORTS = {
  the_library: "import { ReactFlow } from '@xyflow/react';\nexport const f = ReactFlow;",
  its_stylesheet: "import '@xyflow/react/dist/base.css';\nexport const f = 1;",
  its_core: "import { Position } from '@xyflow/system';\nexport const p = Position;",
  its_layout: "import dagre from '@dagrejs/dagre';\nexport const d = dagre;",
  its_graph: "import { Graph } from '@dagrejs/graphlib';\nexport const g = Graph;",
  its_d3: "import { zoom } from 'd3-zoom';\nexport const z = zoom;",
};

// The canvas library's internal store: a state store no code here may take
// on, inside the canvas or out — a second store escalates (AGENTS.md
// § Conventions).
const STORE = {
  root: "import { create } from 'zustand';\nexport const c = create;",
  subpath: "import { createStore } from 'zustand/vanilla';\nexport const c = createStore;",
};

describe('the canvas library’s import rule', () => {
  it.each(Object.entries(IMPORTS))('refuses %s outside src/canvas/', async (_, code) => {
    for (const file of ['src/App.tsx', 'src/shell/Example.tsx', 'src/api/example.ts', 'design/components/Example/Example.tsx']) {
      const messages = await lint(code, file);
      expect(messages.map((m) => m.ruleId), file).toEqual(['no-restricted-imports']);
      expect(messages[0]?.message).toContain('src/canvas/');
    }
  });

  it.each(Object.entries(IMPORTS))('accepts %s inside src/canvas/', async (_, code) => {
    expect(await lint(code, 'src/canvas/Example.tsx')).toEqual([]);
  });

  it.each(Object.entries(STORE))('refuses the store (%s import) everywhere, src/canvas/ included', async (_, code) => {
    for (const file of ['src/App.tsx', 'src/canvas/Example.tsx', 'src/api/example.ts', 'design/components/Example/Example.tsx']) {
      const messages = await lint(code, file);
      expect(messages.map((m) => m.ruleId), file).toEqual(['no-restricted-imports']);
      expect(messages[0]?.message).toContain('decision');
    }
  });

  it('still refuses the test doubles inside src/canvas/', async () => {
    const messages = await lint("import { mockApi } from '../api/testing.ts';\nexport const m = mockApi;", 'src/canvas/Example.tsx');
    expect(messages.map((m) => m.ruleId)).toEqual(['no-restricted-imports']);
  });

  it('lets a screen import the Canvas itself', async () => {
    expect(await lint("import { Canvas } from '../canvas/index.ts';\nexport const c = Canvas;", 'src/shell/Example.tsx')).toEqual([]);
  });
});
