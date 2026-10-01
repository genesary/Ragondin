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

  it('still refuses the test doubles inside src/canvas/', async () => {
    const messages = await lint("import { mockApi } from '../api/testing.ts';\nexport const m = mockApi;", 'src/canvas/Example.tsx');
    expect(messages.map((m) => m.ruleId)).toEqual(['no-restricted-imports']);
  });

  it('lets a screen import the Canvas itself', async () => {
    expect(await lint("import { Canvas } from '../canvas/index.ts';\nexport const c = Canvas;", 'src/shell/Example.tsx')).toEqual([]);
  });
});
