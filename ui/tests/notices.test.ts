// The third-party notices `npm run build` writes into dist/, which the binary
// embeds and serves with the rest of the UI (ARCHITECTURE.md § The
// third-party notices). MIT, ISC and BSD-3-Clause each require their notice to
// accompany a copy, so a bundled package without one is a build failure, not
// a warning.
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { afterEach, describe, expect, it } from 'vitest';
import { bundledOutside, fontNotices, missingNotices, NOTICES_FILE, noticeEntries, renderNotices } from '../scripts/notices.mjs';

const UI = fileURLToPath(new URL('..', import.meta.url));

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function scratch(prefix: string): string {
  const root = mkdtempSync(join(tmpdir(), prefix));
  roots.push(root);
  return root;
}

type Entry = { lock?: Record<string, unknown>; files?: Record<string, string>; installed?: boolean };

/** A throwaway project: a v3 lockfile and, for each installed entry, its package.json and files. */
function project(entries: Record<string, Entry>): string {
  const root = scratch('ragondin-ui-notices-');
  const packages: Record<string, unknown> = { '': { name: 'fixture' } };
  for (const [name, entry] of Object.entries(entries)) {
    const key = `node_modules/${name}`;
    packages[key] = { version: '1.0.0', license: 'MIT', ...entry.lock };
    if (entry.installed === false) continue;
    const bare = name;
    mkdirSync(join(root, key), { recursive: true });
    writeFileSync(join(root, key, 'package.json'), JSON.stringify({ name: bare, version: '1.0.0', license: 'MIT', ...entry.lock }));
    for (const [file, text] of Object.entries(entry.files ?? { LICENSE: `licence of ${bare}` })) writeFileSync(join(root, key, file), text);
  }
  writeFileSync(join(root, 'package-lock.json'), JSON.stringify({ lockfileVersion: 3, packages }));
  return root;
}

/** The runtime packages of ui/package-lock.json, by name. */
function lockfileRuntime(): string[] {
  const lock = JSON.parse(readFileSync(join(UI, 'package-lock.json'), 'utf8')) as { packages: Record<string, { dev?: boolean; devOptional?: boolean }> };
  return Object.entries(lock.packages)
    .filter(([key, entry]) => key.includes('node_modules/') && entry.dev !== true && entry.devOptional !== true)
    .map(([key]) => key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length))
    .sort();
}

describe('noticeEntries', () => {
  it('lists the runtime packages, with name, version, licence and licence text', () => {
    const root = project({ a: {}, '@scope/b': { lock: { license: 'ISC' }, files: { 'LICENSE.md': 'ISC text' } } });
    expect(noticeEntries(root)).toEqual([
      { key: 'node_modules/@scope/b', name: '@scope/b', version: '1.0.0', license: 'ISC', text: 'ISC text' },
      { key: 'node_modules/a', name: 'a', version: '1.0.0', license: 'MIT', text: 'licence of a' },
    ]);
  });

  it('leaves out development packages, which never reach the bundle', () => {
    const root = project({ lib: {}, tool: { lock: { dev: true } }, types: { lock: { devOptional: true } } });
    expect(noticeEntries(root).map((e) => e.name)).toEqual(['lib']);
  });

  it('adds a development package the build itself writes code from, when asked', () => {
    const root = project({ lib: {}, vite: { lock: { dev: true } } });
    expect(noticeEntries(root, ['node_modules/vite']).map((e) => e.name)).toEqual(['lib', 'vite']);
  });

  it.each(['LICENSE', 'LICENSE.md', 'LICENCE.txt', 'license', 'COPYING', 'LICENSE-MIT', 'LICENSE.APACHE2', 'license-apache.txt'])('reads the licence text from %s', (file) => {
    const root = project({ a: { files: { [file]: 'the text' } } });
    expect(noticeEntries(root)[0]?.text).toBe('the text');
  });

  it('skips an optional package this machine did not install: it cannot have been bundled', () => {
    const root = project({ a: {}, 'native-linux': { lock: { optional: true }, installed: false } });
    expect(noticeEntries(root).map((e) => e.name)).toEqual(['a']);
  });

  it('fails on a runtime package with no licence file, naming it', () => {
    const root = project({ a: {}, bare: { files: { 'README.md': 'no licence here' } } });
    expect(() => noticeEntries(root)).toThrow(/bare@1\.0\.0/);
  });

  it('fails on a runtime package that is not installed', () => {
    const root = project({ gone: { installed: false } });
    expect(() => noticeEntries(root)).toThrow(/gone@1\.0\.0/);
  });

  it('covers every runtime package of the real lockfile', () => {
    expect(noticeEntries(UI).map((e) => e.name).sort()).toEqual(lockfileRuntime());
  });
});

describe('bundledOutside', () => {
  const root = '/p/ui';
  const listed = new Set(['node_modules/react', 'node_modules/@xyflow/react', 'node_modules/vite']);

  it('accepts own code and modules of listed packages, query and virtual prefix aside', () => {
    const ids = [
      '/p/ui/index.html',
      '/p/ui/src/main.tsx',
      '/p/ui/node_modules/react/index.js',
      '\0/p/ui/node_modules/react/index.js?commonjs-module',
      '/p/ui/node_modules/@xyflow/react/dist/esm/index.js',
      '\0vite/modulepreload-polyfill.js',
      '\0commonjsHelpers.js',
    ];
    expect(bundledOutside(ids, root, listed)).toEqual([]);
  });

  it('names a module of a package with no notice', () => {
    expect(bundledOutside(['/p/ui/node_modules/vitest/dist/index.js'], root, listed)).toEqual(['node_modules/vitest (/p/ui/node_modules/vitest/dist/index.js)']);
  });

  it('resolves a nested package to its own lockfile key', () => {
    expect(bundledOutside(['/p/ui/node_modules/react/node_modules/@s/x/a.js'], root, listed)).toEqual(['node_modules/react/node_modules/@s/x (/p/ui/node_modules/react/node_modules/@s/x/a.js)']);
  });

  it('names a virtual module it cannot attribute', () => {
    expect(bundledOutside(['\0some-plugin:helper'], root, listed)).toEqual(['an unattributed virtual module (some-plugin:helper)']);
  });

  it('names a file outside ui/ that no package owns', () => {
    expect(bundledOutside(['/elsewhere/x.js'], root, listed)).toEqual(['a file outside ui/ (/elsewhere/x.js)']);
  });
});

describe('fontNotices', () => {
  it('gives each font licence text once, with the files it covers', () => {
    const fonts = fontNotices(UI);
    expect(fonts.map((f) => f.file).sort()).toEqual(['OFL-AtkinsonHyperlegibleMono.txt', 'OFL-WixMadefor.txt']);
    const wix = fonts.find((f) => f.file === 'OFL-WixMadefor.txt');
    expect(wix?.license).toBe('OFL-1.1');
    expect(wix?.fonts).toContain('WixMadeforText-Regular.woff2');
    expect(wix?.text).toBe(readFileSync(join(UI, 'design/fonts/OFL-WixMadefor.txt'), 'utf8'));
  });
});

describe('renderNotices and missingNotices', () => {
  const entries = [
    { key: 'node_modules/a', name: 'a', version: '1.0.0', license: 'MIT', text: 'MIT text of a' },
    { key: 'node_modules/b', name: 'b', version: '2.0.0', license: 'ISC', text: 'ISC text of b' },
  ];
  const fonts = [{ file: 'OFL.txt', license: 'OFL-1.1', fonts: ['A.woff2'], text: 'OFL text' }];

  it('writes each package and font with its licence text', () => {
    const text = renderNotices(entries, fonts);
    for (const piece of ['a 1.0.0', 'MIT', 'MIT text of a', 'b 2.0.0', 'ISC', 'ISC text of b', 'A.woff2', 'OFL-1.1', 'OFL text']) {
      expect(text).toContain(piece);
    }
  });

  it('finds nothing missing in what it wrote', () => {
    expect(missingNotices(renderNotices(entries, fonts), entries, fonts)).toEqual([]);
  });

  it('names a package whose entry is absent', () => {
    expect(missingNotices(renderNotices([entries[0]!], fonts), entries, fonts)).toEqual(['b@2.0.0']);
  });

  it('names a package whose licence text is not the one it ships', () => {
    const edited = renderNotices(entries, fonts).replace('ISC text of b', 'something else');
    expect(missingNotices(edited, entries, fonts)).toEqual(['b@2.0.0']);
  });

  it('names a font licence text that is absent', () => {
    expect(missingNotices(renderNotices(entries, []), entries, fonts)).toEqual(['font licence OFL.txt']);
  });
});

describe('the production build', () => {
  it(`writes ${NOTICES_FILE} with a notice for every runtime package and font`, async () => {
    const outDir = scratch('ragondin-ui-dist-');
    await build({ root: UI, configFile: join(UI, 'vite.config.ts'), logLevel: 'silent', build: { outDir, emptyOutDir: true } });

    const written = readFileSync(join(outDir, NOTICES_FILE), 'utf8');
    expect(missingNotices(written, noticeEntries(UI), fontNotices(UI))).toEqual([]);
    // The modulepreload polyfill and the CommonJS helpers are Vite's own code,
    // written into the bundle: its notice ships too.
    expect(written).toContain(`vite ${JSON.parse(readFileSync(join(UI, 'node_modules/vite/package.json'), 'utf8')).version}`);
  }, 60_000);
});

describe('the gate', () => {
  it('npm run check re-reads the notices after the build has written them', () => {
    const pkg = JSON.parse(readFileSync(join(UI, 'package.json'), 'utf8')) as { scripts: Record<string, string> };
    expect(pkg.scripts.notices).toBe('node scripts/check-notices.mjs');
    const steps = pkg.scripts.check?.split(' && ') ?? [];
    expect(steps.indexOf('npm run build')).toBeGreaterThan(-1);
    expect(steps.indexOf('npm run notices')).toBeGreaterThan(steps.indexOf('npm run build'));
  });
});
