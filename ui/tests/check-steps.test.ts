// `just check-ui` depends on `just build-ui`, which has already built `dist/`,
// and runs the rest of `npm run check` through scripts/check-steps.mjs. These
// tests pin what that script prints: every step but the build, in order, and a
// refusal, rather than a silent second build, when the build is not a step.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const SCRIPT = fileURLToPath(new URL('../scripts/check-steps.mjs', import.meta.url));

/** Runs the script over a package.json whose `check` script is `check`. */
function stepsOf(check: string) {
  const manifest = join(mkdtempSync(join(tmpdir(), 'ragondin-check-steps-')), 'package.json');
  writeFileSync(manifest, JSON.stringify({ scripts: { check } }));
  return spawnSync(process.execPath, [SCRIPT, manifest], { encoding: 'utf8' });
}

describe('check-steps', () => {
  it('prints every step of the check script but the build, in order', () => {
    const run = stepsOf('npm run lint && npm run build && npm run notices');

    expect(run.status).toBe(0);
    expect(run.stdout.trim()).toBe('npm run lint && npm run notices');
  });

  it('fails, naming the recipe to update, when the build is not a step', () => {
    const run = stepsOf('npm run lint && npm run notices');

    expect(run.status).not.toBe(0);
    expect(run.stdout).toBe('');
    expect(run.stderr).toContain('no longer runs npm run build as a step; update check-ui in the justfile');
  });

  it('reads ui/package.json when given no argument', () => {
    const run = spawnSync(process.execPath, [SCRIPT], { encoding: 'utf8' });

    expect(run.status).toBe(0);
    expect(run.stdout).toContain('npm run notices');
    expect(run.stdout.split(' && ').map((s) => s.trim())).not.toContain('npm run build');
  });
});
