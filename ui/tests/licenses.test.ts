import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { LICENSE_ALLOW, LICENSE_EXCEPTIONS } from '../scripts/audit-policy.mjs';
import { auditLicenses, satisfies, unmatchedLicenseExceptions } from '../scripts/licenses.mjs';

const REPO_ROOT = fileURLToPath(new URL('../..', import.meta.url));

/** The `allow` array of deny.toml's `[licenses]` table, comments stripped. */
function denyTomlAllow(): string[] {
  const text = readFileSync(join(REPO_ROOT, 'deny.toml'), 'utf8');
  const uncommented = text
    .split('\n')
    .map((line) => line.replace(/#.*$/, ''))
    .join('\n');
  const section = uncommented.split(/^\[licenses\]\s*$/m)[1]?.split(/^\[/m)[0];
  const array = section?.match(/^allow\s*=\s*\[([^\]]*)\]/m)?.[1];
  if (array === undefined) throw new Error('deny.toml has no [licenses] allow array');
  return [...array.matchAll(/"([^"]+)"/g)].map((m) => m[1] as string);
}

describe('the licence allow list', () => {
  it("equals deny.toml's [licenses] allow, entry for entry", () => {
    const deny = denyTomlAllow();
    expect(deny.length).toBeGreaterThan(0);
    expect([...LICENSE_ALLOW].sort()).toEqual([...deny].sort());
  });
});

describe('satisfies', () => {
  const allow = new Set(['MIT', 'Apache-2.0', 'BSD-3-Clause']);

  it.each([
    ['MIT', true],
    ['GPL-3.0-only', false],
    ['(MIT OR GPL-3.0-only)', true],
    ['GPL-3.0-only OR Apache-2.0', true],
    ['MIT AND BSD-3-Clause', true],
    ['MIT AND GPL-3.0-only', false],
    ['(MIT OR Apache-2.0) AND GPL-3.0-only', false],
    ['(MIT OR Apache-2.0) AND BSD-3-Clause', true],
    ['Apache-2.0 WITH LLVM-exception', false],
    ['SEE LICENSE IN LICENSE.txt', false],
    ['UNLICENSED', false],
    ['', false],
    ['(MIT', false],
  ])('%j is %s', (expression, expected) => {
    expect(satisfies(expression, allow)).toBe(expected);
  });
});

describe('auditLicenses over an installed tree', () => {
  const roots: string[] = [];
  afterEach(() => {
    for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
  });

  type Entry = { lock: Record<string, unknown>; installed?: Record<string, unknown> };

  /** A throwaway project: a v3 lockfile and, for each installed entry, its package.json. */
  function fixture(entries: Record<string, Entry>): string {
    const root = mkdtempSync(join(tmpdir(), 'ragondin-ui-licences-'));
    roots.push(root);
    const packages: Record<string, unknown> = { '': { name: 'fixture' } };
    for (const [name, entry] of Object.entries(entries)) {
      const key = `node_modules/${name}`;
      packages[key] = { version: '1.0.0', ...entry.lock };
      if (entry.installed !== undefined) {
        mkdirSync(join(root, key), { recursive: true });
        writeFileSync(join(root, key, 'package.json'), JSON.stringify({ name, version: '1.0.0', ...entry.installed }));
      }
    }
    writeFileSync(join(root, 'package-lock.json'), JSON.stringify({ lockfileVersion: 3, packages }));
    return root;
  }

  const policy = { allow: ['MIT', 'Apache-2.0'], exceptions: [] };

  it('passes a tree whose every licence is allowed', () => {
    const root = fixture({
      a: { lock: { license: 'MIT' }, installed: { license: 'MIT' } },
      b: { lock: { license: 'MIT OR Apache-2.0' }, installed: { license: 'MIT OR Apache-2.0' } },
    });
    expect(auditLicenses(root, policy)).toEqual([]);
  });

  it('fails a dependency whose licence is outside the allow list', () => {
    const root = fixture({
      a: { lock: { license: 'MIT' }, installed: { license: 'MIT' } },
      copyleft: { lock: { license: 'GPL-3.0-only' }, installed: { license: 'GPL-3.0-only' } },
    });
    expect(auditLicenses(root, policy)).toEqual([
      { package: 'copyleft', version: '1.0.0', license: 'GPL-3.0-only', problem: 'not allowed' },
    ]);
  });

  it('fails a dependency with no licence field', () => {
    const root = fixture({ bare: { lock: {}, installed: {} } });
    expect(auditLicenses(root, policy)).toEqual([
      { package: 'bare', version: '1.0.0', license: null, problem: 'no licence field' },
    ]);
  });

  it("reads the installed package.json rather than trusting the lockfile's copy", () => {
    const root = fixture({ drifted: { lock: { license: 'MIT' }, installed: { license: 'GPL-3.0-only' } } });
    expect(auditLicenses(root, policy)).toEqual([
      { package: 'drifted', version: '1.0.0', license: 'GPL-3.0-only', problem: 'not allowed' },
    ]);
  });

  it("falls back to the lockfile for a platform package this machine did not install", () => {
    const root = fixture({
      'native-linux': { lock: { license: 'GPL-3.0-only', optional: true, os: ['linux'] } },
    });
    expect(auditLicenses(root, policy)).toEqual([
      { package: 'native-linux', version: '1.0.0', license: 'GPL-3.0-only', problem: 'not allowed' },
    ]);
  });

  it('names a scoped or nested package by its own name', () => {
    const root = fixture({
      'outer/node_modules/@scope/inner': { lock: { license: 'GPL-3.0-only' }, installed: { license: 'GPL-3.0-only' } },
    });
    expect(auditLicenses(root, policy)[0]?.package).toBe('@scope/inner');
  });

  const exception = {
    package: 'glob-helper',
    license: 'BlueOak-1.0.0',
    date: '2026-09-30',
    reason: 'fixture',
  };

  it('lets a named exception admit its licence for a development-only package', () => {
    const root = fixture({
      'glob-helper': { lock: { license: 'BlueOak-1.0.0', dev: true }, installed: { license: 'BlueOak-1.0.0' } },
    });
    expect(auditLicenses(root, { ...policy, exceptions: [exception] })).toEqual([]);
  });

  it('never lets an exception admit a licence into the runtime tree', () => {
    const root = fixture({
      'glob-helper': { lock: { license: 'BlueOak-1.0.0' }, installed: { license: 'BlueOak-1.0.0' } },
    });
    expect(auditLicenses(root, { ...policy, exceptions: [exception] })).toEqual([
      { package: 'glob-helper', version: '1.0.0', license: 'BlueOak-1.0.0', problem: 'not allowed' },
    ]);
  });

  it('admits only the licence the exception names, for only the package it names', () => {
    const root = fixture({
      'glob-helper': { lock: { license: 'GPL-3.0-only', dev: true }, installed: { license: 'GPL-3.0-only' } },
      other: { lock: { license: 'BlueOak-1.0.0', dev: true }, installed: { license: 'BlueOak-1.0.0' } },
    });
    expect(auditLicenses(root, { ...policy, exceptions: [exception] }).map((p) => p.package)).toEqual([
      'glob-helper',
      'other',
    ]);
  });

  it('names an exception that admits nothing, so it gets deleted', () => {
    const root = fixture({
      'glob-helper': { lock: { license: 'MIT', dev: true }, installed: { license: 'MIT' } },
    });
    const absent = { ...exception, package: 'gone' };
    expect(unmatchedLicenseExceptions(root, { ...policy, exceptions: [exception, absent] })).toEqual([
      exception,
      absent,
    ]);
  });

  it('does not name an exception that is still needed', () => {
    const root = fixture({
      'glob-helper': { lock: { license: 'BlueOak-1.0.0', dev: true }, installed: { license: 'BlueOak-1.0.0' } },
    });
    expect(unmatchedLicenseExceptions(root, { ...policy, exceptions: [exception] })).toEqual([]);
  });

  it('refuses an exception that does not carry a date and a reason', () => {
    const root = fixture({});
    const bad = { package: 'glob-helper', license: 'BlueOak-1.0.0' };
    expect(() => auditLicenses(root, { ...policy, exceptions: [bad as typeof exception] })).toThrow(/date/);
  });
});

describe('the committed licence exceptions', () => {
  it('each carry a package, a licence, a date and a reason', () => {
    for (const e of LICENSE_EXCEPTIONS) {
      expect(e.package).toMatch(/\S/);
      expect(e.license).toMatch(/\S/);
      expect(e.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(e.reason).toMatch(/\S/);
    }
  });
});
