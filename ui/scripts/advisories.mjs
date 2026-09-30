// The advisory half of the dependency audit: reads an `npm audit --json`
// report and decides what blocks. npm has no ignore list of its own, which is
// the only reason this file exists.

/** @typedef {import('./audit-policy.mjs').AdvisoryException} AdvisoryException */
/** @typedef {{ id: string, severity: string, title: string, package: string, url: string }} Advisory */

const BLOCKING = new Set(['high', 'critical']);

/**
 * Every advisory the report carries, once each, keyed by its GHSA id.
 *
 * @param {unknown} report
 * @returns {Map<string, Advisory>}
 */
function advisoriesIn(report) {
  const r = /** @type {{ auditReportVersion?: unknown, vulnerabilities?: unknown }} */ (report ?? {});
  if (r.auditReportVersion !== 2 || typeof r.vulnerabilities !== 'object' || r.vulnerabilities === null) {
    throw new Error(`unrecognised npm audit report (auditReportVersion ${String(r.auditReportVersion)})`);
  }
  /** @type {Map<string, Advisory>} */
  const found = new Map();
  for (const vulnerability of Object.values(r.vulnerabilities)) {
    for (const via of vulnerability?.via ?? []) {
      // A string names another vulnerable package, whose own entry carries
      // the advisory.
      if (typeof via !== 'object' || via === null) continue;
      const url = String(via.url ?? '');
      const id = url.match(/GHSA(-[0-9a-z]{4}){3}/)?.[0] ?? `npm-${String(via.source)}`;
      if (!found.has(id)) {
        found.set(id, {
          id,
          severity: String(via.severity),
          title: String(via.title ?? ''),
          package: String(via.name ?? ''),
          url,
        });
      }
    }
  }
  return found;
}

/**
 * The advisories at high or critical severity that no exception names.
 *
 * @param {unknown} report
 * @param {readonly AdvisoryException[]} exceptions
 * @returns {Advisory[]}
 */
export function blockingAdvisories(report, exceptions) {
  const excepted = new Set(exceptions.map((e) => e.id));
  return [...advisoriesIn(report).values()].filter((a) => BLOCKING.has(a.severity) && !excepted.has(a.id));
}

/**
 * The exceptions whose advisory the report no longer carries: each is an
 * allowance to delete.
 *
 * @param {unknown} report
 * @param {readonly AdvisoryException[]} exceptions
 * @returns {AdvisoryException[]}
 */
export function unmatchedExceptions(report, exceptions) {
  const present = advisoriesIn(report);
  return exceptions.filter((e) => !present.has(e.id));
}

/**
 * Throws unless every exception carries an id, a date, a reason and the issue
 * that removes it.
 *
 * @param {readonly Record<string, unknown>[]} exceptions
 */
export function validateAdvisoryExceptions(exceptions) {
  for (const e of exceptions) {
    for (const field of ['id', 'date', 'reason', 'removedBy']) {
      const value = e[field];
      if (typeof value !== 'string' || value.trim() === '') {
        throw new Error(`advisory exception ${JSON.stringify(e)} has no ${field}`);
      }
    }
    if (!/^\d{4}-\d{2}-\d{2}$/.test(String(e.date))) {
      throw new Error(`advisory exception ${String(e.id)}: date must be YYYY-MM-DD`);
    }
    if (!/^#\d+$/.test(String(e.removedBy))) {
      throw new Error(`advisory exception ${String(e.id)}: removedBy must name an issue, as #<number>`);
    }
  }
}
