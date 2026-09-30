import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { LICENSE_ALLOW } from '../scripts/audit-policy.mjs';
import { auditFonts, parseFontManifest } from '../scripts/font-licenses.mjs';

const UI = fileURLToPath(new URL('..', import.meta.url));
const allow = new Set(['OFL-1.1', 'MIT']);

const sha = (bytes: string) => createHash('sha256').update(bytes).digest('hex');

function manifest(fonts: string[], texts: string[]): string {
  return [
    '# Fonts',
    '',
    '## Files',
    '',
    '| File | Family | Weight | Licence | Licence text | SHA-256 |',
    '|---|---|---|---|---|---|',
    ...fonts,
    '',
    '## Licence texts',
    '',
    '| File | SHA-256 |',
    '|---|---|',
    ...texts,
    '',
  ].join('\n');
}

const fontRow = (file: string, content: string, license = 'OFL-1.1', text = 'OFL.txt') => `| \`${file}\` | A | 400 | ${license} | \`${text}\` | \`${sha(content)}\` |`;
const textRow = (file: string, content: string) => `| \`${file}\` | \`${sha(content)}\` |`;

describe('parseFontManifest', () => {
  it('reads one entry per font row and one per licence-text row', () => {
    const parsed = parseFontManifest(manifest(['| `A.woff2` | Family A | 400 | OFL-1.1 | `OFL-A.txt` | `abc` |'], ['| `OFL-A.txt` | `def` |']));
    expect(parsed.fonts).toEqual([{ file: 'A.woff2', family: 'Family A', license: 'OFL-1.1', licenseFile: 'OFL-A.txt', sha256: 'abc' }]);
    expect(parsed.texts).toEqual([{ file: 'OFL-A.txt', sha256: 'def' }]);
  });

  it('ignores tables whose first cell is neither a font nor a licence text', () => {
    expect(parseFontManifest('| Family | Licence |\n|---|---|\n| A | MIT |\n')).toEqual({ fonts: [], texts: [] });
  });
});

describe('auditFonts', () => {
  const roots: string[] = [];
  afterEach(() => {
    for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
  });

  /** A throwaway ui/ tree: paths are relative to it. */
  function ui(files: Record<string, string>): string {
    const root = mkdtempSync(join(tmpdir(), 'ui-'));
    roots.push(root);
    mkdirSync(join(root, 'design/fonts'), { recursive: true });
    for (const [name, content] of Object.entries(files)) {
      mkdirSync(dirname(join(root, name)), { recursive: true });
      writeFileSync(join(root, name), content);
    }
    return root;
  }
  const good = { 'design/fonts/A.woff2': 'font-a', 'design/fonts/OFL.txt': 'licence' };
  const goodManifest = manifest([fontRow('A.woff2', 'font-a')], [textRow('OFL.txt', 'licence')]);

  it('passes a font whose row names an allowed licence, its pinned text and its digest', () => {
    expect(auditFonts(ui({ ...good, 'design/fonts/LICENSES.md': goodManifest }), allow)).toEqual([]);
  });

  it('fails a font file that has no row', () => {
    const root = ui({ ...good, 'design/fonts/B.ttf': 'font-b', 'design/fonts/LICENSES.md': goodManifest });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/B.ttf', problem: 'has no entry in LICENSES.md' }]);
  });

  it('fails every font when LICENSES.md is missing', () => {
    expect(auditFonts(ui({ 'design/fonts/A.otf': 'font-a' }), allow)).toEqual([{ file: 'design/fonts/A.otf', problem: 'has no entry in LICENSES.md' }]);
  });

  it.each(['design/fonts/sub/B.woff2', 'src/B.woff2', 'public/B.woff', 'design/B.ttf'])('fails a font at %s: every font lives directly in ui/design/fonts/', (path) => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': goodManifest, [path]: 'font-b' });
    expect(auditFonts(root, allow)).toEqual([{ file: path, problem: 'is a font file outside ui/design/fonts/' }]);
  });

  it('does not look inside node_modules or dist', () => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': goodManifest, 'node_modules/x/F.woff2': 'f', 'dist/assets/A-1.woff2': 'f' });
    expect(auditFonts(root, allow)).toEqual([]);
  });

  it('fails a licence off the allow list', () => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': manifest([fontRow('A.woff2', 'font-a', 'GPL-3.0-only')], [textRow('OFL.txt', 'licence')]) });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/A.woff2', problem: 'licence GPL-3.0-only is not on the allow list' }]);
  });

  it('fails a row whose licence text is absent', () => {
    const root = ui({ 'design/fonts/A.woff2': 'font-a', 'design/fonts/LICENSES.md': manifest([fontRow('A.woff2', 'font-a')], [textRow('OFL.txt', 'licence')]) });
    expect(auditFonts(root, allow)).toContainEqual({ file: 'design/fonts/A.woff2', problem: 'licence text OFL.txt is absent' });
  });

  it('fails a licence text that is not pinned by a digest', () => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': manifest([fontRow('A.woff2', 'font-a')], []) });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/A.woff2', problem: 'licence text OFL.txt has no SHA-256 in LICENSES.md' }]);
  });

  it('fails a licence text that differs from its pinned digest', () => {
    const root = ui({ ...good, 'design/fonts/OFL.txt': 'edited', 'design/fonts/LICENSES.md': goodManifest });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/OFL.txt', problem: 'differs from the SHA-256 recorded in LICENSES.md' }]);
  });

  it.each(['../OFL.txt', 'sub/OFL.txt', '/etc/OFL.txt'])('refuses a licence text at %s: it must sit beside the fonts', (text) => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': manifest([fontRow('A.woff2', 'font-a', 'OFL-1.1', text)], [textRow('OFL.txt', 'licence')]) });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/A.woff2', problem: `licence text ${text} is outside ui/design/fonts/` }]);
  });

  it('fails a file that differs from the digest recorded for it', () => {
    const root = ui({ ...good, 'design/fonts/A.woff2': 'edited', 'design/fonts/LICENSES.md': goodManifest });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/A.woff2', problem: 'differs from the SHA-256 recorded in LICENSES.md' }]);
  });

  it('fails a font listed twice, and a licence text listed twice', () => {
    const root = ui({ ...good, 'design/fonts/LICENSES.md': manifest([fontRow('A.woff2', 'font-a'), fontRow('A.woff2', 'font-a')], [textRow('OFL.txt', 'licence'), textRow('OFL.txt', 'licence')]) });
    expect(auditFonts(root, allow)).toEqual([
      { file: 'design/fonts/A.woff2', problem: 'is listed more than once in LICENSES.md' },
      { file: 'design/fonts/OFL.txt', problem: 'is listed more than once in LICENSES.md' },
    ]);
  });

  it('fails a row whose file is absent, so the manifest cannot go stale', () => {
    const root = ui({ 'design/fonts/OFL.txt': 'licence', 'design/fonts/LICENSES.md': manifest([fontRow('Gone.woff2', 'x')], [textRow('OFL.txt', 'licence')]) });
    expect(auditFonts(root, allow)).toEqual([{ file: 'design/fonts/Gone.woff2', problem: 'is listed in LICENSES.md but absent' }]);
  });
});

describe('the committed fonts', () => {
  it('every font file under ui/ is in ui/design/fonts/, declared under a licence on the allow list', () => {
    expect(auditFonts(UI, new Set(LICENSE_ALLOW))).toEqual([]);
  });

  it('there are fonts and pinned licence texts to audit, so the check above cannot pass vacuously', () => {
    const parsed = parseFontManifest(readFileSync(join(UI, 'design/fonts/LICENSES.md'), 'utf8'));
    expect(parsed.fonts.length).toBe(9);
    expect(parsed.texts.length).toBe(2);
  });
});

describe('the gate', () => {
  it('npm run check runs the font audit, through npm run audit', () => {
    const pkg = JSON.parse(readFileSync(join(UI, 'package.json'), 'utf8')) as { scripts: Record<string, string> };
    expect(pkg.scripts.audit).toContain('node scripts/check-font-licenses.mjs');
    expect(pkg.scripts.check).toContain('npm run audit');
  });
});
