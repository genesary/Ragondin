// The UI's build identity is baked into the bundle at build time and compared
// with the one the API reports (ADR-C36 § 1). It is useful only if both sides
// compute it by one rule: the binary's crate version, the commit, and whether
// the tree differs from it, as `<version>+<12-hex commit>[-dirty]`, or
// `<version>+unknown` outside a checkout. The git half of that rule is one
// file both builds read, bin/ragondin/build-identity.rule.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { buildIdentity } from '../scripts/build-identity.mjs';
import config from '../vite.config.ts';

const REPO = fileURLToPath(new URL('../..', import.meta.url));
const RULE = 'bin/ragondin/build-identity.rule';

/** A repository root holding the two manifests the binary's version is read from, and the rule. */
function fixture(binVersion: string, workspaceVersion = '1.2.3') {
  const root = mkdtempSync(join(tmpdir(), 'ragondin-build-'));
  writeFileSync(join(root, 'Cargo.toml'), `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${workspaceVersion}"\nedition = "2021"\n`);
  mkdirSync(join(root, 'bin/ragondin'), { recursive: true });
  writeFileSync(join(root, 'bin/ragondin/Cargo.toml'), `[package]\nname = "ragondin"\n${binVersion}\n\n[dependencies]\nversion = "9.9.9"\n`);
  copyFileSync(join(REPO, RULE), join(root, RULE));
  return root;
}

/** git in `root`, isolated from the machine's configuration. */
function git(root: string, ...args: string[]) {
  return execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' },
  }).trim();
}

/** The fixture as a checkout with everything committed, and its 12-hex commit. */
function checkout() {
  const root = fixture('version.workspace = true');
  git(root, 'init', '-q');
  git(root, 'add', '.');
  git(root, 'commit', '-q', '-m', 'fixture');
  return { root, head: git(root, 'rev-parse', '--short=12', 'HEAD') };
}

describe('buildIdentity', () => {
  it('is the workspace version the binary inherits, and the 12-hex commit', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root, () => ({ commit: '0123456789ab', dirty: false }))).toBe('1.2.3+0123456789ab');
  });

  it('is the binary’s own version when it declares one', () => {
    const root = fixture('version = "4.5.6"');
    expect(buildIdentity(root, () => ({ commit: '0123456789ab', dirty: false }))).toBe('4.5.6+0123456789ab');
  });

  it('appends `-dirty` when the tree differs from the commit, as the binary does', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root, () => ({ commit: '0123456789ab', dirty: true }))).toBe('1.2.3+0123456789ab-dirty');
  });

  it('names the commit `unknown` when no commit can be read', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root, () => null)).toBe('1.2.3+unknown');
  });

  it('reads the commit with git by default: what `git rev-parse --short=12 HEAD` prints', () => {
    const head = execFileSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: REPO, encoding: 'utf8' }).trim();
    expect(buildIdentity(REPO)).toMatch(new RegExp(`^\\d+\\.\\d+\\.\\d+[^+]*\\+${head}(-dirty)?$`));
  });

  it('refuses a manifest it cannot read a version from, rather than baking a wrong identity', () => {
    const root = fixture('');
    expect(() => buildIdentity(root, () => ({ commit: 'x', dirty: false }))).toThrow(/version/);
  });
});

describe('the rule, read from the file the binary’s build script reads', () => {
  it('calls a checkout with nothing changed clean', () => {
    const { root, head } = checkout();
    expect(buildIdentity(root)).toBe(`1.2.3+${head}`);
  });

  it('leaves the identity clean when an untracked file no build reads appears', () => {
    const { root, head } = checkout();
    writeFileSync(join(root, '.DS_Store'), 'Finder');
    expect(buildIdentity(root)).toBe(`1.2.3+${head}`);
  });

  it('marks the identity dirty when a tracked file is modified', () => {
    const { root, head } = checkout();
    writeFileSync(join(root, 'Cargo.toml'), '# edited\n', { flag: 'a' });
    expect(buildIdentity(root)).toBe(`1.2.3+${head}-dirty`);
  });

  it('marks the identity dirty when a new file is staged', () => {
    const { root, head } = checkout();
    writeFileSync(join(root, 'new.rs'), '');
    git(root, 'add', 'new.rs');
    expect(buildIdentity(root)).toBe(`1.2.3+${head}-dirty`);
  });

  it('names the commit `unknown`, and nothing dirty, outside a checkout', () => {
    const root = fixture('version.workspace = true');
    expect(buildIdentity(root)).toBe('1.2.3+unknown');
  });
});

describe('the bundle', () => {
  it('bakes the identity in at build time, from the build and never from a response', () => {
    expect(config.define?.['__RAGONDIN_BUILD__']).toBe(JSON.stringify(buildIdentity(REPO)));
  });
});
