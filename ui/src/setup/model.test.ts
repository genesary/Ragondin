import { describe, expect, it } from 'vitest';
import type { BenchmarkEntry } from '../api/types.ts';
import { formatSize, groundTruthLabel, isFirstLaunch, shortDigest, smallestAvailable, splitBuild } from './model.ts';

const entry = (name: string, state: BenchmarkEntry['state']): BenchmarkEntry => ({ name, format: 'beir', ground_truth: null, licence: null, licence_url: null, state });

describe('formatSize', () => {
  it.each([
    [512, '512 B'],
    [5_200_000, '5.2 MB'],
    [17_149_999, '17.1 MB'],
    [148_000_000, '148 MB'],
    [2_340_000_000, '2.3 GB'],
    [1_000, '1 kB'],
  ])('writes %d bytes as %s, in decimal units', (bytes, text) => {
    expect(formatSize(bytes)).toBe(text);
  });
});

describe('shortDigest', () => {
  it('is the first twelve characters, as the rest of the UI shortens a hash', () => {
    expect(shortDigest('0123456789abcdef')).toBe('0123456789ab');
  });
});

describe('groundTruthLabel', () => {
  it('says what a benchmark carries in words', () => {
    expect(groundTruthLabel('qrels')).toBe('qrels');
    expect(groundTruthLabel('reference_answers')).toBe('reference answers');
    expect(groundTruthLabel('both')).toBe('qrels and reference answers');
    expect(groundTruthLabel('none')).toBe('no ground truth');
  });
});

describe('isFirstLaunch', () => {
  it('is a workspace with no benchmark on disk and no service', () => {
    expect(isFirstLaunch([], [])).toBe(true);
    // What the manifest offers is not on disk yet.
    expect(isFirstLaunch([entry('beir/scifact', { kind: 'available', size_bytes: 1 })], [])).toBe(true);
  });

  it('is over once a benchmark is on disk in any state, or a service is bound', () => {
    expect(isFirstLaunch([entry('a', { kind: 'ready', dataset_version: 'x' })], [])).toBe(false);
    expect(isFirstLaunch([entry('a', { kind: 'differs', expected: 'x', found: 'y' })], [])).toBe(false);
    expect(isFirstLaunch([], [{ family: 'generator', name: 'qwen', uri: 'u', connected: false, identity: null }])).toBe(false);
  });
});

describe('smallestAvailable', () => {
  it('is the available benchmark with the fewest bytes, or none', () => {
    const big = entry('big', { kind: 'available', size_bytes: 9 });
    const small = entry('small', { kind: 'available', size_bytes: 3 });
    expect(smallestAvailable([big, entry('ready', { kind: 'ready', dataset_version: 'x' }), small])).toBe(small);
    expect(smallestAvailable([entry('ready', { kind: 'ready', dataset_version: 'x' })])).toBeNull();
  });
});

describe('splitBuild', () => {
  it('reads the version and the commit out of the build identity', () => {
    expect(splitBuild('0.1.0+aaaaaaaaaaaa')).toEqual({ version: '0.1.0', commit: 'aaaaaaaaaaaa' });
    expect(splitBuild('0.1.0')).toEqual({ version: '0.1.0', commit: null });
  });
});
