// ui/DEPENDENCIES.md is the `[workspace.dependencies]` rule transposed: every
// dependency package.json declares has a row, and no row outlives its
// dependency. A table that can drift from the manifest is not a record.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const UI_ROOT = fileURLToPath(new URL('..', import.meta.url));
const manifest = JSON.parse(readFileSync(`${UI_ROOT}/package.json`, 'utf8')) as {
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
};
const doc = readFileSync(`${UI_ROOT}/DEPENDENCIES.md`, 'utf8');

/** The package names in the first column of the table under `## <heading>`. */
function tableUnder(heading: string): string[] {
  const section = doc.split(new RegExp(`^## ${heading}\\s*$`, 'm'))[1]?.split(/^## /m)[0];
  if (section === undefined) throw new Error(`DEPENDENCIES.md has no "## ${heading}" section`);
  return section
    .split('\n')
    .filter((line) => line.startsWith('|'))
    .slice(2) // header and separator
    .map((line) => line.split('|')[1]?.trim().replace(/^`|`$/g, '') ?? '');
}

describe('ui/DEPENDENCIES.md', () => {
  it('has one row per runtime dependency, and no other', () => {
    const declared = Object.keys(manifest.dependencies ?? {}).sort();
    expect(declared.length).toBeGreaterThan(0);
    expect(tableUnder('Runtime dependencies').sort()).toEqual(declared);
  });

  it('has one row per development dependency, and no other', () => {
    expect(tableUnder('Development dependencies').sort()).toEqual(Object.keys(manifest.devDependencies ?? {}).sort());
  });

  it('gives every runtime dependency a role and a reason', () => {
    const section = doc.split(/^## Runtime dependencies\s*$/m)[1]?.split(/^## /m)[0] ?? '';
    const rows = section.split('\n').filter((line) => line.startsWith('|')).slice(2);
    for (const row of rows) {
      const cells = row.split('|').slice(1, -1).map((c) => c.trim());
      expect(cells, row).toHaveLength(3);
      for (const cell of cells) expect(cell, row).toMatch(/\S/);
    }
  });
});
