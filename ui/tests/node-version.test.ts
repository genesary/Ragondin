// The Node major is pinned in .node-version, which CI reads, and mirrored by
// `engines`, which `npm ci` enforces; the two must name the same major.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const UI_ROOT = fileURLToPath(new URL('..', import.meta.url));

describe('the pinned Node major', () => {
  it('is the same in .node-version and in package.json engines', () => {
    const pinned = readFileSync(`${UI_ROOT}/.node-version`, 'utf8').trim();
    const manifest = JSON.parse(readFileSync(`${UI_ROOT}/package.json`, 'utf8')) as { engines: { node: string } };
    expect(pinned).toMatch(/^\d+$/);
    expect(manifest.engines.node).toBe(`>=${pinned} <${Number(pinned) + 1}`);
  });
});
