// The third-party notices of the bundle: one file in dist/, written by the
// build, which the binary embeds and serves with the rest of the UI
// (ARCHITECTURE.md § The third-party notices). MIT, ISC and BSD-3-Clause each
// require the copyright and permission notice to accompany a copy, and the
// binary redistributes the bundle. Read from the installed tree, as the
// licence audit is, rather than by a third-party generator.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { isAbsolute, join, relative, sep } from 'node:path';
import { parseFontManifest, FONTS_DIR } from './font-licenses.mjs';

/** The file the build writes into dist/, and the binary serves at `/<NOTICES_FILE>`. */
export const NOTICES_FILE = 'third-party-notices.txt';

/**
 * The virtual modules the build tool writes its own code into the bundle
 * from, by id (the leading `\0` removed), and the package that code is from.
 * A virtual module not named here fails the build, so a new one is looked at
 * rather than shipped without a notice.
 */
const VIRTUAL = new Map([
  ['vite/modulepreload-polyfill.js', 'node_modules/vite'],
  ['vite/preload-helper.js', 'node_modules/vite'],
  // @rollup/plugin-commonjs's helpers, which Vite bundles; its LICENSE.md
  // carries that plugin's notice.
  ['commonjsHelpers.js', 'node_modules/vite'],
]);

/** The development packages whose code the build writes into the bundle: the targets of VIRTUAL. */
export const BUILD_INJECTED = [...new Set(VIRTUAL.values())];

/** A package's licence file: LICENSE, LICENCE or COPYING, with or without an extension, any case. */
const LICENSE_FILE = /^(licen[cs]e|copying)(\.[a-z]+)?$/i;

/**
 * @typedef {{ key: string, name: string, version: string, license: string, text: string }} NoticeEntry
 * @typedef {{ file: string, license: string, fonts: string[], text: string }} FontNotice
 */

const nameOf = (/** @type {string} */ key) => key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);

/**
 * The licence field of a package.json, the SPDX string or the old object
 * form's `type`.
 *
 * @param {{ license?: unknown }} manifest
 * @returns {string}
 */
function licenseOf(manifest) {
  const { license } = manifest;
  if (typeof license === 'string') return license;
  if (typeof license === 'object' && license !== null && 'type' in license && typeof license.type === 'string') return license.type;
  return 'UNKNOWN';
}

/**
 * One notice per runtime package of the lockfile — every package that is not
 * development-only, so the whole closure `dependencies` can bundle — plus the
 * development packages named in `extra`, sorted by lockfile key. A runtime
 * package not installed, or installed with no licence file, throws: its
 * notice cannot be written, and shipping without it is what this prevents.
 * An optional package this machine did not install is skipped, since the
 * build cannot have bundled it.
 *
 * @param {string} root ui/
 * @param {readonly string[]} [extra] lockfile keys of development packages to include
 * @returns {NoticeEntry[]}
 */
export function noticeEntries(root, extra = []) {
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  if (lock.lockfileVersion !== 3 || typeof lock.packages !== 'object') {
    throw new Error('package-lock.json is not a lockfile v3 with a "packages" map');
  }
  /** @type {NoticeEntry[]} */
  const entries = [];
  /** @type {string[]} */
  const problems = [];
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key.includes('node_modules/') || entry.link === true) continue;
    const runtime = entry.dev !== true && entry.devOptional !== true;
    if (!runtime && !extra.includes(key)) continue;
    const dir = join(root, key);
    const manifestPath = join(dir, 'package.json');
    if (!existsSync(manifestPath)) {
      if (entry.optional === true) continue;
      problems.push(`${nameOf(key)}@${entry.version}: not installed (run npm ci)`);
      continue;
    }
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    const version = String(manifest.version ?? entry.version);
    const file = readdirSync(dir).filter((name) => LICENSE_FILE.test(name)).sort()[0];
    if (file === undefined) {
      problems.push(`${nameOf(key)}@${version}: no LICENSE file in ${key}`);
      continue;
    }
    entries.push({ key, name: nameOf(key), version, license: licenseOf(manifest), text: readFileSync(join(dir, file), 'utf8') });
  }
  if (problems.length > 0) {
    throw new Error(`The third-party notices cannot be written:\n  ${problems.join('\n  ')}`);
  }
  return entries.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

/**
 * The lockfile key of the package that owns `path`: everything up to and
 * including its last `node_modules/<name>` or `node_modules/@scope/name`.
 *
 * @param {string} path relative to ui/, with `/` separators
 * @returns {string | null}
 */
function packageKeyOf(path) {
  const at = path.lastIndexOf('node_modules/');
  if (at === -1) return null;
  const rest = path.slice(at + 'node_modules/'.length).split('/');
  const name = rest[0]?.startsWith('@') ? rest.slice(0, 2).join('/') : rest[0];
  return `${path.slice(0, at)}node_modules/${name}`;
}

/**
 * Every module of the build, by id, that comes from neither `ui/`'s own code
 * nor a package with a notice in `listed` — each as a line naming it. A
 * leading `\0` (a plugin's marker for a module it made) and a `?query` are
 * removed before the id is read.
 *
 * @param {Iterable<string>} ids the module ids of the build's graph
 * @param {string} root ui/, absolute
 * @param {ReadonlySet<string>} listed lockfile keys of the packages the notices cover
 * @returns {string[]}
 */
export function bundledOutside(ids, root, listed) {
  /** @type {string[]} */
  const problems = [];
  for (const id of ids) {
    const path = id.replace(/^\0/, '').replace(/\?.*$/, '');
    if (!isAbsolute(path)) {
      const key = VIRTUAL.get(path);
      if (key === undefined) problems.push(`an unattributed virtual module (${path})`);
      else if (!listed.has(key)) problems.push(`${key} (${path})`);
      continue;
    }
    const inside = relative(root, path).split(sep).join('/');
    if (inside.startsWith('../')) {
      problems.push(`a file outside ui/ (${path})`);
      continue;
    }
    const key = packageKeyOf(inside);
    if (key !== null && !listed.has(key)) problems.push(`${key} (${path})`);
  }
  return [...new Set(problems)];
}

/**
 * One notice per font licence text in ui/design/fonts/LICENSES.md, with the
 * font files it covers.
 *
 * @param {string} root ui/
 * @returns {FontNotice[]}
 */
export function fontNotices(root) {
  const dir = join(root, FONTS_DIR);
  const { fonts, texts } = parseFontManifest(readFileSync(join(dir, 'LICENSES.md'), 'utf8'));
  return texts.map(({ file }) => {
    const covered = fonts.filter((font) => font.licenseFile === file);
    return {
      file,
      license: [...new Set(covered.map((font) => font.license))].join(', '),
      fonts: covered.map((font) => font.file),
      text: readFileSync(join(dir, file), 'utf8'),
    };
  });
}

const RULE = '='.repeat(80);
const THIN = '-'.repeat(80);

/** @param {NoticeEntry} entry */
const packageBlock = (entry) => `${RULE}\n${entry.name} ${entry.version}\nLicence: ${entry.license}\n${THIN}\n${entry.text.trimEnd()}\n`;

/** @param {FontNotice} font */
const fontBlock = (font) => `${RULE}\nFont licence ${font.file}\nLicence: ${font.license}\nCovers: ${font.fonts.join(', ')}\n${THIN}\n${font.text.trimEnd()}\n`;

/**
 * The notices file: a header, then one block per package, then one per font
 * licence text.
 *
 * @param {readonly NoticeEntry[]} entries
 * @param {readonly FontNotice[]} fonts
 * @returns {string}
 */
export function renderNotices(entries, fonts) {
  const header = [
    'Third-party notices of the ragondin UI',
    '',
    'ragondin is licensed under Apache-2.0 (the LICENSE file of its repository).',
    'The UI that `ragondin ui` serves bundles the third-party software and fonts',
    'below; each is distributed under the licence reproduced with it.',
    '',
  ].join('\n');
  return [header, ...entries.map(packageBlock), ...fonts.map(fontBlock)].join('\n');
}

/**
 * Every package or font licence whose block, exactly as renderNotices writes
 * it — name, version, licence and text — is absent from `text`.
 *
 * @param {string} text the notices file
 * @param {readonly NoticeEntry[]} entries
 * @param {readonly FontNotice[]} fonts
 * @returns {string[]}
 */
export function missingNotices(text, entries, fonts) {
  return [
    ...entries.filter((entry) => !text.includes(packageBlock(entry))).map((entry) => `${entry.name}@${entry.version}`),
    ...fonts.filter((font) => !text.includes(fontBlock(font))).map((font) => `font licence ${font.file}`),
  ];
}

/**
 * The build step: writes NOTICES_FILE into the bundle, and fails the build
 * when a module of the bundle comes from a package the notices do not cover.
 *
 * @param {string} root ui/, absolute
 * @returns {import('vite').Plugin}
 */
export function thirdPartyNotices(root) {
  return {
    name: 'ragondin-third-party-notices',
    apply: 'build',
    generateBundle() {
      const entries = noticeEntries(root, BUILD_INJECTED);
      const outside = bundledOutside(this.getModuleIds(), root, new Set(entries.map((entry) => entry.key)));
      if (outside.length > 0) {
        this.error(`The bundle holds code no third-party notice covers (ui/ARCHITECTURE.md § The third-party notices):\n  ${outside.join('\n  ')}`);
      }
      this.emitFile({ type: 'asset', fileName: NOTICES_FILE, source: renderNotices(entries, fontNotices(root)) });
    },
  };
}
