// The licence half of the dependency audit. Short on purpose, rather than a
// third-party checker: it is the whole of the policy's enforcement, and it has
// to be read to be trusted.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * @typedef {{ package: string, version: string, license: string | null, problem: 'not allowed' | 'no licence field' }} LicenseProblem
 */

/**
 * Whether an SPDX licence expression can be satisfied from `allowed`: `OR`
 * needs one side, `AND` both. A `WITH` exception, a parse failure, or a string
 * that is not an expression at all (`SEE LICENSE IN …`, `UNLICENSED`) is not
 * satisfiable, so it fails rather than being guessed at.
 *
 * @param {string} expression
 * @param {ReadonlySet<string>} allowed
 * @returns {boolean}
 */
export function satisfies(expression, allowed) {
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  let at = 0;
  const OPERATORS = new Set(['AND', 'OR', 'WITH', '(', ')']);

  /** @returns {boolean | null} null when the expression does not parse */
  function atom() {
    const token = tokens[at++];
    if (token === undefined) return null;
    if (token === '(') {
      const inner = or();
      if (tokens[at++] !== ')') return null;
      return inner;
    }
    if (OPERATORS.has(token)) return null;
    if (tokens[at] === 'WITH') {
      at += 2;
      if (tokens[at - 1] === undefined) return null;
      // An exception changes the licence's terms; nothing on the list names
      // one, so it is refused rather than reduced to its base licence.
      return false;
    }
    return allowed.has(token);
  }

  /** @param {'AND' | 'OR'} op @param {() => boolean | null} operand */
  function chain(op, operand) {
    let value = operand();
    while (value !== null && tokens[at] === op) {
      at++;
      const next = operand();
      if (next === null) return null;
      value = op === 'AND' ? value && next : value || next;
    }
    return value;
  }

  const and = () => chain('AND', atom);
  const or = () => chain('OR', and);

  const value = or();
  return value === true && at === tokens.length;
}

/**
 * The licence field of an installed package.json: the SPDX string, or the
 * `type` of the old object form. Anything else counts as absent.
 *
 * @param {unknown} manifest
 * @returns {string | null}
 */
function licenseOf(manifest) {
  if (typeof manifest !== 'object' || manifest === null) return null;
  const { license } = /** @type {{ license?: unknown }} */ (manifest);
  if (typeof license === 'string') return license;
  if (typeof license === 'object' && license !== null) {
    const { type } = /** @type {{ type?: unknown }} */ (license);
    if (typeof type === 'string') return type;
  }
  return null;
}

/**
 * Every third-party package of the project at `root`, with its licence.
 *
 * The package list is the lockfile's, so a platform-specific package this
 * machine did not install is still audited, from the licence npm recorded for
 * it; every package that is installed is read from its own package.json, which
 * is what the tree actually carries.
 *
 * @param {string} root
 * @returns {{ name: string, version: string, license: string | null }[]}
 */
function packagesOf(root) {
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  if (lock.lockfileVersion !== 3 || typeof lock.packages !== 'object') {
    throw new Error('package-lock.json is not a lockfile v3 with a "packages" map');
  }
  const packages = [];
  for (const [key, entry] of Object.entries(lock.packages)) {
    // The root project, and workspace links, are this repository's own code.
    if (!key.includes('node_modules/') || entry.link === true) continue;
    const manifestPath = join(root, key, 'package.json');
    const installed = existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, 'utf8')) : null;
    packages.push({
      name: key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length),
      version: String(installed?.version ?? entry.version),
      license: installed !== null ? licenseOf(installed) : licenseOf(entry),
    });
  }
  return packages;
}

/**
 * Every package of the project at `root` whose licence the allow list does not
 * admit, runtime and development packages alike.
 *
 * @param {string} root
 * @param {{ allow: readonly string[] }} policy
 * @returns {LicenseProblem[]}
 */
export function auditLicenses(root, policy) {
  const allow = new Set(policy.allow);

  /** @type {LicenseProblem[]} */
  const problems = [];
  for (const { name, version, license } of packagesOf(root)) {
    if (license === null) {
      problems.push({ package: name, version, license: null, problem: 'no licence field' });
    } else if (!satisfies(license, allow)) {
      problems.push({ package: name, version, license, problem: 'not allowed' });
    }
  }
  return problems;
}
