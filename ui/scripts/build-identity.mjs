// The build identity the UI bakes into its bundle (vite.config.ts `define`),
// computed from the source the binary computes its own from: the `ragondin`
// crate's version, which it inherits from `[workspace.package]` unless it
// declares one, and the commit `git rev-parse --short=12 HEAD` names — or
// `unknown` outside a checkout. `<version>+<commit>`, the form the binary's
// build script writes. ui/ARCHITECTURE.md § The build identity handshake says
// why a commit and what happens when the two differ.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The `[section]` of a TOML file, as its raw text up to the next section.
 * @param {string} toml
 * @param {string} section
 */
function sectionOf(toml, section) {
  const start = toml.indexOf(`[${section}]\n`);
  if (start === -1) return '';
  const rest = toml.slice(start + section.length + 3);
  const end = rest.search(/^\[/m);
  return end === -1 ? rest : rest.slice(0, end);
}

/**
 * @param {string} body
 * @returns {string | null}
 */
const versionIn = (body) => body.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1] ?? null;

/**
 * The commit HEAD names, abbreviated to 12 hex digits, or null.
 * @param {string} root
 * @returns {string | null}
 */
function gitCommit(root) {
  try {
    return execFileSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    // Not a checkout, or no git: the binary names the commit `unknown` then,
    // and so must the UI, or the two builds would never compare equal.
    return null;
  }
}

/**
 * @param {string} root The repository root.
 * @param {(root: string) => string | null} [commit] How the commit is read.
 * @returns {string}
 */
export function buildIdentity(root, commit = gitCommit) {
  const bin = sectionOf(readFileSync(join(root, 'bin/ragondin/Cargo.toml'), 'utf8'), 'package');
  const version = /^version\.workspace\s*=\s*true\s*$/m.test(bin)
    ? versionIn(sectionOf(readFileSync(join(root, 'Cargo.toml'), 'utf8'), 'workspace.package'))
    : versionIn(bin);
  if (version === null) throw new Error(`no version for the ragondin crate in ${root}: the build identity cannot be computed`);
  return `${version}+${commit(root) ?? 'unknown'}`;
}
