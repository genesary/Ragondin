/** @vitest-environment happy-dom */
// What the editor remembers for the browser session follows a pipeline renamed: the rewrite choice and the fork.
import { beforeEach, describe, expect, it } from 'vitest';
import { chooseRewrite, forkedFrom, rememberFork, renameSession, rewriteChosen } from './session.ts';

beforeEach(() => sessionStorage.clear());

describe('renameSession', () => {
  it('carries the rewrite choice and the run forked from to the new name', () => {
    chooseRewrite('old');
    rememberFork('old', 'run-1');
    renameSession('old', 'new');
    expect(rewriteChosen('new')).toBe(true);
    expect(forkedFrom('new')).toBe('run-1');
  });

  it('gives the new name nothing the old one did not have', () => {
    renameSession('old', 'new');
    expect(rewriteChosen('new')).toBe(false);
    expect(forkedFrom('new')).toBeNull();
  });
});
