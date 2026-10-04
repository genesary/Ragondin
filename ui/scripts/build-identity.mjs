// The build identity the UI bakes into its bundle (vite.config.ts `define`),
// computed by the rule the binary computes its own by: the `ragondin` crate's
// version, which it inherits from `[workspace.package]` unless it declares
// one, then the commit and whether the tree differs from it, as the git
// commands in bin/ragondin/build-identity.rule say — the file the binary's
// build script reads too. `<version>+<commit>[-dirty]`, or
// `<version>+unknown` outside a checkout: the form the binary reports.
// ui/ARCHITECTURE.md § The build identity handshake says why a commit and what
// happens when the two differ.
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
 * The git commands of the build identity's rule, each as its arguments: the
 * file's non-comment lines, the commit's first, the dirty check's second.
 * @param {string} root
 * @returns {string[][]}
 */
function ruleOf(root) {
  const lines = readFileSync(join(root, 'bin/ragondin/build-identity.rule'), 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith('#'));
  if (lines.length !== 2) throw new Error(`bin/ragondin/build-identity.rule in ${root} holds ${lines.length} commands, not 2`);
  return lines.map((line) => line.split(/\s+/));
}

/**
 * One git command's trimmed output, or null when git is absent, fails, or
 * prints nothing.
 * @param {string} root
 * @param {string[]} args
 * @returns {string | null}
 */
function git(root, args) {
  try {
    const out = execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    return out === '' ? null : out;
  } catch {
    return null;
  }
}

/**
 * The commit and whether the tree differs from it, by the rule; or null
 * outside a checkout, where the binary names the commit `unknown` too, or the
 * two builds would never compare equal.
 * @param {string} root
 * @returns {{ commit: string, dirty: boolean } | null}
 */
function gitState(root) {
  const [commitArgs = [], dirtyArgs = []] = ruleOf(root);
  const commit = git(root, commitArgs);
  return commit === null ? null : { commit, dirty: git(root, dirtyArgs) !== null };
}

/**
 * @param {string} root The repository root.
 * @param {(root: string) => { commit: string, dirty: boolean } | null} [state] How the commit and the tree's state are read.
 * @returns {string}
 */
export function buildIdentity(root, state = gitState) {
  const bin = sectionOf(readFileSync(join(root, 'bin/ragondin/Cargo.toml'), 'utf8'), 'package');
  const version = /^version\.workspace\s*=\s*true\s*$/m.test(bin)
    ? versionIn(sectionOf(readFileSync(join(root, 'Cargo.toml'), 'utf8'), 'workspace.package'))
    : versionIn(bin);
  if (version === null) throw new Error(`no version for the ragondin crate in ${root}: the build identity cannot be computed`);
  const read = state(root);
  if (read === null) return `${version}+unknown`;
  return `${version}+${read.commit}${read.dirty ? '-dirty' : ''}`;
}
