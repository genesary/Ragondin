// The UI's build identity is baked into the bundle at build time and compared
// with the one the API reports (ADR-C36 § 1). It is useful only if both sides
// compute it from the same source: the binary's crate version and the commit,
// as `<version>+<12-hex commit>`, or `<version>+unknown` outside a checkout.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { buildIdentity } from '../scripts/build-identity.mjs';
import config from '../vite.config.ts';

const REPO = fileURLToPath(new URL('../..', import.meta.url));

/** A repository root holding the two manifests the binary's version is read from. */
function fixture(binVersion: string, workspaceVersion = '1.2.3') {
  const root = mkdtempSync(join(tmpdir(), 'ragondin-build-'));
  writeFileSync(join(root, 'Cargo.toml'), `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${workspaceVersion}"\nedition = "2021"\n`);
  mkdirSync(join(root, 'bin/ragondin'), { recursive: true });
  writeFileSync(join(root, 'bin/ragondin/Cargo.toml'), `[package]\nname = "ragondin"\n${binVersion}\n\n[dependencies]\nversion = "9.9.9"\n`);
  return root;
}

describe('buildIdentity', () => {
  it('is the workspace version the binary inherits, and the 12-hex commit', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root, () => '0123456789ab')).toBe('1.2.3+0123456789ab');
  });

  it('is the binary’s own version when it declares one', () => {
    const root = fixture('version = "4.5.6"');
    expect(buildIdentity(root, () => '0123456789ab')).toBe('4.5.6+0123456789ab');
  });

  it('names the commit `unknown` when no commit can be read', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root, () => null)).toBe('1.2.3+unknown');
  });

  it('reads the commit with git by default: what `git rev-parse --short=12 HEAD` prints', () => {
    const head = execFileSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: REPO, encoding: 'utf8' }).trim();
    expect(buildIdentity(REPO)).toMatch(new RegExp(`^\\d+\\.\\d+\\.\\d+[^+]*\\+${head}$`));
  });

  it('refuses a manifest it cannot read a version from, rather than baking a wrong identity', () => {
    const root = fixture('');
    expect(() => buildIdentity(root, () => 'x')).toThrow(/version/);
  });
});

describe('the bundle', () => {
  it('bakes the identity in at build time, from the build and never from a response', () => {
    expect(config.define?.['__RAGONDIN_BUILD__']).toBe(JSON.stringify(buildIdentity(REPO)));
  });
});
