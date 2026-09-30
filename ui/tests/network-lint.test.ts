// The network-confinement rule of eslint.config.js, tested against the real
// configuration: the same source is linted as if it lived outside and inside
// `src/api/`, and only the first must fail. ARCHITECTURE.md § The network lint
// says what the rule is for and what it cannot see.
import { ESLint, type Linter } from 'eslint';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const UI_ROOT = fileURLToPath(new URL('..', import.meta.url));
const eslint = new ESLint({ cwd: UI_ROOT });

const OUTSIDE = join(UI_ROOT, 'src/views/Example.ts');
const INSIDE = join(UI_ROOT, 'src/api/example.ts');

async function lint(code: string, filePath: string): Promise<Linter.LintMessage[]> {
  const [result] = await eslint.lintText(code, { filePath });
  if (result === undefined) throw new Error(`ESLint returned no result for ${filePath}`);
  return result.messages;
}

// Each sample uses its primitive once and exports it, so no other rule
// (unused variables, say) has anything to report.
const SAMPLES: Record<string, string> = {
  fetch: "export const r = fetch('/api/v1/runs');",
  XMLHttpRequest: 'export const r = new XMLHttpRequest();',
  WebSocket: "export const r = new WebSocket('/api/v1/events');",
  EventSource: "export const r = new EventSource('/api/v1/events');",
  'window.fetch': "export const r = window.fetch('/api/v1/runs');",
  'globalThis.fetch': "export const r = globalThis.fetch('/api/v1/runs');",
  "self['EventSource']": "export const r = new self['EventSource']('/api/v1/events');",
};

describe('network confinement', () => {
  for (const [primitive, code] of Object.entries(SAMPLES)) {
    it(`rejects ${primitive} outside src/api/, naming the rule`, async () => {
      const messages = await lint(code, OUTSIDE);
      expect(messages).toHaveLength(1);
      const [message] = messages;
      expect(message?.severity).toBe(2);
      expect(message?.ruleId).toMatch(/^no-restricted-(globals|properties)$/);
      expect(message?.message).toContain('src/api/');
    });

    it(`accepts ${primitive} inside src/api/`, async () => {
      expect(await lint(code, INSIDE)).toEqual([]);
    });
  }

  it('leaves an unrelated method that happens to be called fetch alone', async () => {
    const code = 'export const r = (cache: { fetch(): number }) => cache.fetch();';
    expect(await lint(code, OUTSIDE)).toEqual([]);
  });
});
