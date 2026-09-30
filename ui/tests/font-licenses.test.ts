import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { LICENSE_ALLOW } from '../scripts/audit-policy.mjs';
import { auditFonts, parseFontManifest } from '../scripts/font-licenses.mjs';

const FONTS = fileURLToPath(new URL('../design/fonts/', import.meta.url));
const allow = new Set(['OFL-1.1', 'MIT']);

const sha = (bytes: string) => createHash('sha256').update(bytes).digest('hex');

function manifest(rows: string[]): string {
  return [
    '# Fonts',
    '',
    '## Files',
    '',
    '| File | Family | Weight | Licence | Licence text | SHA-256 |',
    '|---|---|---|---|---|---|',
    ...rows,
    '',
  ].join('\n');
}

describe('parseFontManifest', () => {
  it('reads one entry per table row naming a file', () => {
    const rows = parseFontManifest(manifest(['| `A.woff2` | Family A | 400 | OFL-1.1 | `OFL-A.txt` | `abc` |']));
    expect(rows).toEqual([{ file: 'A.woff2', family: 'Family A', license: 'OFL-1.1', licenseFile: 'OFL-A.txt', sha256: 'abc' }]);
  });

  it('ignores tables whose first cell is not a file name', () => {
    expect(parseFontManifest('| Family | Licence |\n|---|---|\n| A | MIT |\n')).toEqual([]);
  });
});

describe('auditFonts', () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  function fontsDir(files: Record<string, string>): string {
    const dir = mkdtempSync(join(tmpdir(), 'fonts-'));
    dirs.push(dir);
    for (const [name, content] of Object.entries(files)) writeFileSync(join(dir, name), content);
    return dir;
  }

  it('passes a font whose entry names an allowed licence, its text and its digest', () => {
    const dir = fontsDir({
      'A.woff2': 'font-a',
      'OFL-A.txt': 'licence',
      'LICENSES.md': manifest([`| \`A.woff2\` | A | 400 | OFL-1.1 | \`OFL-A.txt\` | \`${sha('font-a')}\` |`]),
    });
    expect(auditFonts(dir, allow)).toEqual([]);
  });

  it('fails a font file that has no entry', () => {
    const dir = fontsDir({ 'A.woff2': 'font-a', 'B.ttf': 'font-b', 'OFL-A.txt': 'l', 'LICENSES.md': manifest([`| \`A.woff2\` | A | 400 | OFL-1.1 | \`OFL-A.txt\` | \`${sha('font-a')}\` |`]) });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'B.ttf', problem: 'has no entry in LICENSES.md' }]);
  });

  it('fails every font when LICENSES.md is missing', () => {
    const dir = fontsDir({ 'A.otf': 'font-a' });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'A.otf', problem: 'has no entry in LICENSES.md' }]);
  });

  it('fails a licence off the allow list', () => {
    const dir = fontsDir({ 'A.woff2': 'font-a', 'L.txt': 'l', 'LICENSES.md': manifest([`| \`A.woff2\` | A | 400 | GPL-3.0-only | \`L.txt\` | \`${sha('font-a')}\` |`]) });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'A.woff2', problem: 'licence GPL-3.0-only is not on the allow list' }]);
  });

  it('fails an entry whose licence text is absent', () => {
    const dir = fontsDir({ 'A.woff2': 'font-a', 'LICENSES.md': manifest([`| \`A.woff2\` | A | 400 | OFL-1.1 | \`OFL-A.txt\` | \`${sha('font-a')}\` |`]) });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'A.woff2', problem: 'licence text OFL-A.txt is absent' }]);
  });

  it('fails a file that differs from the digest recorded for it', () => {
    const dir = fontsDir({ 'A.woff2': 'edited', 'L.txt': 'l', 'LICENSES.md': manifest([`| \`A.woff2\` | A | 400 | OFL-1.1 | \`L.txt\` | \`${sha('font-a')}\` |`]) });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'A.woff2', problem: 'differs from the SHA-256 recorded in LICENSES.md' }]);
  });

  it('fails an entry whose file is absent, so the manifest cannot go stale', () => {
    const dir = fontsDir({ 'L.txt': 'l', 'LICENSES.md': manifest(['| `Gone.woff2` | A | 400 | OFL-1.1 | `L.txt` | `00` |']) });
    expect(auditFonts(dir, allow)).toEqual([{ file: 'Gone.woff2', problem: 'is listed in LICENSES.md but absent' }]);
  });
});

describe('the committed fonts', () => {
  it('every font file under ui/design/fonts/ is declared under a licence on the allow list', () => {
    expect(auditFonts(FONTS, new Set(LICENSE_ALLOW))).toEqual([]);
  });

  it('there are fonts to audit, so the check above cannot pass vacuously', () => {
    const rows = parseFontManifest(readFileSync(join(FONTS, 'LICENSES.md'), 'utf8'));
    expect(rows.length).toBeGreaterThan(0);
  });
});

describe('the gate', () => {
  it('npm run check runs the font audit, through npm run audit', () => {
    const pkg = JSON.parse(readFileSync(fileURLToPath(new URL('../package.json', import.meta.url)), 'utf8')) as { scripts: Record<string, string> };
    expect(pkg.scripts.audit).toContain('node scripts/check-font-licenses.mjs');
    expect(pkg.scripts.check).toContain('npm run audit');
  });
});
