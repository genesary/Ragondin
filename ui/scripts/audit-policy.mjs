// The dependency audit's policy for `ui/`: which licences the npm tree may
// carry, and which advisories are knowingly let through. It is deny.toml
// transposed, since the two toolchains cannot share one file. The format of
// each list, and why each exists, is in ui/ARCHITECTURE.md § The dependency audit.

/**
 * The licences any package in the tree may carry.
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
  'CDLA-Permissive-2.0',
  'ISC',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'Zlib',
];

/**
 * A licence outside the allow list, admitted for one named package, and only
 * while that package is a development dependency: the check ignores an
 * exception for anything the bundle ships. The shape of cargo-deny's
 * `[[licenses.exceptions]]`, plus the date and the reason deny.toml asks of
 * every allowance.
 *
 * @typedef {{ package: string, license: string, date: string, reason: string }} LicenseException
 * @type {readonly LicenseException[]}
 */
export const LICENSE_EXCEPTIONS = [
  {
    package: 'minimatch',
    license: 'BlueOak-1.0.0',
    date: '2026-09-30',
    reason:
      'ESLint depends on it unconditionally, so no configuration of ours drops it. ' +
      'The Blue Oak Model License 1.0.0 is permissive (OSI-approved, no reciprocal ' +
      'term), and the package is a development tool that never reaches the bundle.',
  },
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
