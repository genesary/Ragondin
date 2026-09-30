import { describe, expect, it } from 'vitest';
import { ADVISORY_EXCEPTIONS } from '../scripts/audit-policy.mjs';
import { blockingAdvisories, unmatchedExceptions, validateAdvisoryExceptions } from '../scripts/advisories.mjs';

/** An `npm audit --json` report (auditReportVersion 2) reduced to what is read. */
function report(advisories: { pkg: string; id: string; severity: string }[]) {
  const vulnerabilities: Record<string, unknown> = {};
  for (const { pkg, id, severity } of advisories) {
    vulnerabilities[pkg] = {
      name: pkg,
      severity,
      via: [{ source: 1, name: pkg, title: `${id} in ${pkg}`, url: `https://github.com/advisories/${id}`, severity }],
    };
  }
  // A package that is vulnerable only through another names it by string.
  vulnerabilities['dependent'] = { name: 'dependent', severity: 'high', via: advisories.map((a) => a.pkg) };
  return { auditReportVersion: 2, vulnerabilities };
}

const exception = {
  id: 'GHSA-aaaa-bbbb-cccc',
  date: '2026-09-30',
  reason: 'fixture',
  removedBy: '#1',
};

describe('blockingAdvisories', () => {
  it('blocks on high and critical, and not below', () => {
    const blocking = blockingAdvisories(
      report([
        { pkg: 'a', id: 'GHSA-1111-1111-1111', severity: 'critical' },
        { pkg: 'b', id: 'GHSA-2222-2222-2222', severity: 'high' },
        { pkg: 'c', id: 'GHSA-3333-3333-3333', severity: 'moderate' },
        { pkg: 'd', id: 'GHSA-4444-4444-4444', severity: 'low' },
      ]),
      [],
    );
    expect(blocking.map((a) => a.id)).toEqual(['GHSA-1111-1111-1111', 'GHSA-2222-2222-2222']);
  });

  it('lets an exception through, and only the advisory it names', () => {
    const blocking = blockingAdvisories(
      report([
        { pkg: 'a', id: 'GHSA-aaaa-bbbb-cccc', severity: 'high' },
        { pkg: 'b', id: 'GHSA-2222-2222-2222', severity: 'high' },
      ]),
      [exception],
    );
    expect(blocking.map((a) => a.id)).toEqual(['GHSA-2222-2222-2222']);
  });

  it('reports an advisory once however many packages reach it', () => {
    const r = report([{ pkg: 'a', id: 'GHSA-1111-1111-1111', severity: 'high' }]);
    (r.vulnerabilities['a-copy'] as unknown) = r.vulnerabilities['a'];
    expect(blockingAdvisories(r, [])).toHaveLength(1);
  });

  it('refuses a report it does not recognise rather than passing it', () => {
    expect(() => blockingAdvisories({ auditReportVersion: 1 }, [])).toThrow(/auditReportVersion/);
  });
});

describe('unmatchedExceptions', () => {
  it('names an exception whose advisory the report no longer carries', () => {
    const r = report([{ pkg: 'b', id: 'GHSA-2222-2222-2222', severity: 'high' }]);
    expect(unmatchedExceptions(r, [exception])).toEqual([exception]);
  });
});

describe('validateAdvisoryExceptions', () => {
  it('accepts an entry carrying the id, a date, a reason and the issue that removes it', () => {
    expect(() => validateAdvisoryExceptions([exception])).not.toThrow();
  });

  it.each(['id', 'date', 'reason', 'removedBy'])('refuses an entry without %s', (field) => {
    const partial: Record<string, string> = { ...exception };
    delete partial[field];
    expect(() => validateAdvisoryExceptions([partial])).toThrow(field);
  });

  it('refuses a removal that is not an issue reference', () => {
    expect(() => validateAdvisoryExceptions([{ ...exception, removedBy: 'later' }])).toThrow(/removedBy/);
  });

  it('holds for the committed exceptions', () => {
    expect(() => validateAdvisoryExceptions(ADVISORY_EXCEPTIONS)).not.toThrow();
  });
});
