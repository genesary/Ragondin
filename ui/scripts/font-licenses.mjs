// The font half of the licence audit. The npm audit reads the lockfile, and a
// font is not a package: it is a file committed under ui/design/fonts/, so no
// lockfile names it. This reads the manifest committed beside the files,
// ui/design/fonts/LICENSES.md, and holds every font file to the same allow list
// as the npm tree (ui/ARCHITECTURE.md § The dependency audit).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import { satisfies } from './licenses.mjs';

/** Every extension a browser loads as a font. A file with one of them is audited. */
export const FONT_EXTENSIONS = ['.woff2', '.woff', '.ttf', '.otf', '.eot'];

/**
 * @typedef {{ file: string, family: string, license: string, licenseFile: string, sha256: string }} FontEntry
 * @typedef {{ file: string, problem: string }} FontProblem
 */

/** Strips the backticks a Markdown cell wraps a file name or digest in. */
const bare = (/** @type {string} */ cell) => cell.trim().replace(/^`(.*)`$/, '$1');

/**
 * The entries of LICENSES.md: every table row whose first cell is a file name
 * with a font extension, read as File | Family | Weight | Licence | Licence
 * text | SHA-256.
 *
 * @param {string} markdown
 * @returns {FontEntry[]}
 */
export function parseFontManifest(markdown) {
  /** @type {FontEntry[]} */
  const entries = [];
  for (const line of markdown.split('\n')) {
    if (!line.trim().startsWith('|')) continue;
    const cells = line.trim().replace(/^\||\|$/g, '').split('|').map(bare);
    const [file, family, , license, licenseFile, sha256] = cells;
    if (file === undefined || !FONT_EXTENSIONS.includes(extname(file).toLowerCase())) continue;
    entries.push({ file, family: family ?? '', license: license ?? '', licenseFile: licenseFile ?? '', sha256: sha256 ?? '' });
  }
  return entries;
}

/**
 * Every problem with the fonts in `dir`: a font file with no entry, a licence
 * off the allow list, a missing licence text, a file that no longer matches
 * the digest recorded when it was committed, or an entry whose file is gone.
 *
 * @param {string} dir
 * @param {ReadonlySet<string>} allow
 * @returns {FontProblem[]}
 */
export function auditFonts(dir, allow) {
  const manifestPath = join(dir, 'LICENSES.md');
  const entries = existsSync(manifestPath) ? parseFontManifest(readFileSync(manifestPath, 'utf8')) : [];
  const byFile = new Map(entries.map((entry) => [entry.file, entry]));
  const fonts = readdirSync(dir)
    .filter((name) => FONT_EXTENSIONS.includes(extname(name).toLowerCase()))
    .sort();

  /** @type {FontProblem[]} */
  const problems = [];
  for (const file of fonts) {
    const entry = byFile.get(file);
    if (entry === undefined) {
      problems.push({ file, problem: 'has no entry in LICENSES.md' });
      continue;
    }
    if (!satisfies(entry.license, allow)) {
      problems.push({ file, problem: `licence ${entry.license} is not on the allow list` });
    } else if (entry.licenseFile === '' || !existsSync(join(dir, entry.licenseFile))) {
      problems.push({ file, problem: `licence text ${entry.licenseFile} is absent` });
    } else if (createHash('sha256').update(readFileSync(join(dir, file))).digest('hex') !== entry.sha256) {
      problems.push({ file, problem: 'differs from the SHA-256 recorded in LICENSES.md' });
    }
  }
  for (const entry of entries) {
    if (!fonts.includes(entry.file)) problems.push({ file: entry.file, problem: 'is listed in LICENSES.md but absent' });
  }
  return problems;
}
