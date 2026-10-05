/** @vitest-environment happy-dom */
// The recent pipelines, as local storage keeps them: read back whatever is stored, written newest first, renamed in place.
import { beforeEach, describe, expect, it } from 'vitest';
import { RECENT_KEPT, recentPipelines, rememberPipeline, renameRecent } from './recent.ts';

const KEY = 'ragondin.editor.recent';

beforeEach(() => localStorage.clear());

describe('recentPipelines', () => {
  it('reads stored text that is not JSON as none, and is written over by the next pipeline opened', () => {
    localStorage.setItem(KEY, '["hybrid",');
    expect(recentPipelines()).toEqual([]);
    rememberPipeline('rag');
    expect(recentPipelines()).toEqual(['rag']);
  });

  it('keeps only the names of what is stored, and at most the number it keeps', () => {
    localStorage.setItem(KEY, JSON.stringify(['a', 1, 'b', null, 'c', 'd', 'e', 'f']));
    expect(recentPipelines()).toEqual(['a', 'b', 'c', 'd', 'e'].slice(0, RECENT_KEPT));
    localStorage.setItem(KEY, JSON.stringify({ a: 1 }));
    expect(recentPipelines()).toEqual([]);
  });
});

describe('renameRecent', () => {
  it('gives a renamed pipeline its new name in its place, and leaves the others', () => {
    rememberPipeline('c');
    rememberPipeline('b');
    rememberPipeline('a');
    renameRecent('b', 'x');
    expect(recentPipelines()).toEqual(['a', 'x', 'c']);
  });

  it('leaves the list as it was when the renamed pipeline is not in it', () => {
    rememberPipeline('a');
    renameRecent('b', 'x');
    expect(recentPipelines()).toEqual(['a']);
  });
});
