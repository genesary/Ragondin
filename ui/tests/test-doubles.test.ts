// src/api/testing.ts holds the network's test doubles. It must never reach the
// bundle the binary serves: the lint refuses an import of it from anything but
// a test, and the production build is read to check that none of it is there.
// ARCHITECTURE.md § The client.
import { ESLint, type Linter } from 'eslint';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const UI = fileURLToPath(new URL('..', import.meta.url));
const eslint = new ESLint({ cwd: UI });

async function lint(code: string, file: string): Promise<Linter.LintMessage[]> {
  const [result] = await eslint.lintText(code, { filePath: join(UI, file) });
  if (result === undefined) throw new Error(`ESLint returned no result for ${file}`);
  return result.messages;
}

const IMPORTS = {
  relative: "import { mockApi } from './api/testing.ts';\nexport const m = mockApi;",
  deeper: "import { FakeEventSource } from '../api/testing.ts';\nexport const f = FakeEventSource;",
  unsuffixed: "import { mockApi } from '../../src/api/testing';\nexport const m = mockApi;",
};

describe('the test doubles’ import rule', () => {
  it.each(Object.entries(IMPORTS))('refuses a %s import of src/api/testing.ts from application code', async (_, code) => {
    for (const file of ['src/App.tsx', 'src/shell/Example.tsx', 'design/components/Example/Example.tsx']) {
      const messages = await lint(code, file);
      expect(messages.map((m) => m.ruleId)).toEqual(['no-restricted-imports']);
      expect(messages[0]?.message).toContain('tests');
    }
  });

  it('refuses a sibling import from inside src/api/', async () => {
    for (const code of ["import { mockApi } from './testing.ts';\nexport const m = mockApi;", "import { mockApi } from './testing';\nexport const m = mockApi;"]) {
      const messages = await lint(code, 'src/api/client.ts');
      expect(messages.map((m) => m.ruleId)).toEqual(['no-restricted-imports']);
    }
    const nested = await lint("import { mockApi } from '../testing.ts';\nexport const m = mockApi;", 'src/api/sub/example.ts');
    expect(nested.map((m) => m.ruleId)).toEqual(['no-restricted-imports']);
  });

  it('accepts a sibling import from a test inside src/api/', async () => {
    expect(await lint("import { mockApi } from './testing.ts';\nexport const m = mockApi;", 'src/api/client.test.ts')).toEqual([]);
  });

  it.each(['src/App.test.tsx', 'src/shell/Example.test.ts', 'tests/example.test.ts'])('accepts it in %s', async (file) => {
    expect(await lint(IMPORTS.relative, file)).toEqual([]);
  });
});

describe('the production build', () => {
  let out: string;

  beforeAll(async () => {
    out = mkdtempSync(join(tmpdir(), 'ui-dist-'));
    await build({ root: UI, logLevel: 'silent', build: { outDir: out, emptyOutDir: true } });
  }, 60_000);

  afterAll(() => {
    rmSync(out, { recursive: true, force: true });
  });

  it('carries none of the test doubles', () => {
    const scripts = readdirSync(join(out, 'assets')).filter((f) => f.endsWith('.js'));
    expect(scripts.length).toBeGreaterThan(0);
    for (const file of scripts) {
      const code = readFileSync(join(out, 'assets', file), 'utf8');
      // Strings survive minification where names do not.
      expect(code).not.toContain('no EventSource was opened');
      expect(code).not.toContain('in this mock');
    }
  });
});
