// The font half of the licence audit. The npm audit reads the lockfile, and a
// font is not a package: it is a file committed under ui/, so no lockfile
// names it. This walks ui/ for font files, requires each to live in
// ui/design/fonts/, and holds it to the same allow list as the npm tree through
// the manifest committed beside it, ui/design/fonts/LICENSES.md
// (ui/ARCHITECTURE.md § The dependency audit).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join, relative, sep } from 'node:path';
import { satisfies } from './licenses.mjs';

/** Every extension a browser loads as a font. A file with one of them is audited. */
export const FONT_EXTENSIONS = ['.woff2', '.woff', '.ttf', '.otf', '.eot'];

/** Where every font lives, relative to ui/. */
export const FONTS_DIR = 'design/fonts';

/** Directories the walk skips: installed packages (the npm audit's) and build output. */
const SKIP = new Set(['node_modules', 'dist', '.git']);

/**
 * @typedef {{ file: string, family: string, license: string, licenseFile: string, sha256: string }} FontEntry
 * @typedef {{ file: string, sha256: string }} TextEntry
 * @typedef {{ file: string, problem: string }} FontProblem
 */

/** Strips the backticks a Markdown cell wraps a file name or digest in. */
const bare = (/** @type {string} */ cell) => cell.trim().replace(/^`(.*)`$/, '$1');
const isFont = (/** @type {string} */ name) => FONT_EXTENSIONS.includes(extname(name).toLowerCase());
const digest = (/** @type {string} */ path) => createHash('sha256').update(readFileSync(path)).digest('hex');

/**
 * The entries of LICENSES.md. A row whose first cell is a font file reads File
 * | Family | Weight | Licence | Licence text | SHA-256; a row whose first cell
 * is a `.txt` file reads File | SHA-256, and pins a licence text.
 *
 * @param {string} markdown
 * @returns {{ fonts: FontEntry[], texts: TextEntry[] }}
 */
export function parseFontManifest(markdown) {
  /** @type {FontEntry[]} */
  const fonts = [];
  /** @type {TextEntry[]} */
  const texts = [];
  for (const line of markdown.split('\n')) {
    if (!line.trim().startsWith('|')) continue;
    const cells = line.trim().replace(/^\||\|$/g, '').split('|').map(bare);
    const [file] = cells;
    if (file === undefined) continue;
    if (isFont(file)) {
      const [, family, , license, licenseFile, sha256] = cells;
      fonts.push({ file, family: family ?? '', license: license ?? '', licenseFile: licenseFile ?? '', sha256: sha256 ?? '' });
    } else if (extname(file).toLowerCase() === '.txt') {
      texts.push({ file, sha256: cells[1] ?? '' });
    }
  }
  return { fonts, texts };
}

/**
 * Every font file under `dir`, as a path relative to `root`, sorted.
 *
 * @param {string} root
 * @param {string} dir
 * @returns {string[]}
 */
function findFonts(root, dir = root) {
  /** @type {string[]} */
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (!SKIP.has(name)) out.push(...findFonts(root, path));
    } else if (isFont(name)) {
      out.push(relative(root, path).split(sep).join('/'));
    }
  }
  return out.sort();
}

/** Names listed more than once. */
const duplicates = (/** @type {string[]} */ names) => [...new Set(names.filter((n, i) => names.indexOf(n) !== i))];

/**
 * Every problem with the fonts under `root` (ui/): a font file outside
 * design/fonts/, or with no row; a row listed twice; a licence off the allow
 * list; a licence text missing, outside design/fonts/, unpinned, or changed;
 * a font file changed since it was committed; a row whose file is gone.
 *
 * @param {string} root
 * @param {ReadonlySet<string>} allow
 * @returns {FontProblem[]}
 */
export function auditFonts(root, allow) {
  const dir = join(root, FONTS_DIR);
  const at = (/** @type {string} */ name) => `${FONTS_DIR}/${name}`;
  const manifestPath = join(dir, 'LICENSES.md');
  const { fonts: entries, texts } = existsSync(manifestPath) ? parseFontManifest(readFileSync(manifestPath, 'utf8')) : { fonts: [], texts: [] };
  const byFile = new Map(entries.map((entry) => [entry.file, entry]));
  const textByFile = new Map(texts.map((entry) => [entry.file, entry]));
  const found = findFonts(root);

  /** @type {FontProblem[]} */
  const problems = [];
  for (const path of found) {
    const name = path.slice(FONTS_DIR.length + 1);
    if (!path.startsWith(`${FONTS_DIR}/`) || name.includes('/')) {
      problems.push({ file: path, problem: 'is a font file outside ui/design/fonts/' });
      continue;
    }
    const entry = byFile.get(name);
    if (entry === undefined) {
      problems.push({ file: path, problem: 'has no entry in LICENSES.md' });
      continue;
    }
    const text = entry.licenseFile;
    if (!satisfies(entry.license, allow)) {
      problems.push({ file: path, problem: `licence ${entry.license} is not on the allow list` });
    } else if (text === '' || text.includes('/') || text.includes('\\') || text === '..' || text === '.') {
      problems.push({ file: path, problem: `licence text ${text} is outside ui/design/fonts/` });
    } else if (!existsSync(join(dir, text))) {
      problems.push({ file: path, problem: `licence text ${text} is absent` });
    } else if (!textByFile.has(text)) {
      problems.push({ file: path, problem: `licence text ${text} has no SHA-256 in LICENSES.md` });
    } else if (digest(join(dir, name)) !== entry.sha256) {
      problems.push({ file: path, problem: 'differs from the SHA-256 recorded in LICENSES.md' });
    }
  }
  for (const text of texts) {
    const path = join(dir, text.file);
    if (text.file.includes('/') || text.file.includes('\\')) {
      problems.push({ file: at(text.file), problem: 'is a licence text outside ui/design/fonts/' });
    } else if (!existsSync(path)) {
      problems.push({ file: at(text.file), problem: 'is pinned in LICENSES.md but absent' });
    } else if (digest(path) !== text.sha256) {
      problems.push({ file: at(text.file), problem: 'differs from the SHA-256 recorded in LICENSES.md' });
    }
  }
  for (const name of duplicates(entries.map((e) => e.file))) problems.push({ file: at(name), problem: 'is listed more than once in LICENSES.md' });
  for (const name of duplicates(texts.map((e) => e.file))) problems.push({ file: at(name), problem: 'is listed more than once in LICENSES.md' });
  for (const entry of new Set(entries.map((e) => e.file))) {
    if (!found.includes(at(entry))) problems.push({ file: at(entry), problem: 'is listed in LICENSES.md but absent' });
  }
  return problems;
}
