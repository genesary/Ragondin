// The dependency audit's policy for `ui/`: which licences the npm tree may
// carry, and which advisories are knowingly let through. It is deny.toml
// transposed, since the two toolchains cannot share one file. The format of
// each list, and why each exists, is in ui/ARCHITECTURE.md § The dependency audit.

/**
 * The licences any package in the tree may carry, runtime or development
 * alike. There is no per-package exception: a licence outside this list is
 * refused whatever the package's role.
 *
 * A COPY of `[licenses] allow` in the repository root's deny.toml, duplicated
 * on purpose, and it must stay identical: tests/licenses.test.ts reads both and
 * fails when they differ. Change deny.toml first, for the reasons its comments
 * give, and this list in the same change.
 *
 * @type {readonly string[]}
 */
export const LICENSE_ALLOW = [
  'Apache-2.0',
  'MIT',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'BlueOak-1.0.0',
  'CDLA-Permissive-2.0',
  'ISC',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'Zlib',
];

/**
 * An advisory `npm audit` reports at high or critical severity, knowingly let
 * through. Every entry carries the advisory id (its GHSA identifier), the date
 * it was added, the reason it is an allowance rather than a fix, and the issue
 * that removes it again, which is the shape deny.toml's `ignore` comment
 * prescribes. Kept as short as it can be: a lockfile bump is cheaper than an
 * allowance, and an allowance outlives whoever wrote it.
 *
 * @typedef {{ id: string, date: string, reason: string, removedBy: string }} AdvisoryException
 * @type {readonly AdvisoryException[]}
 */
export const ADVISORY_EXCEPTIONS = [];
