/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ApiProblem } from '../api/client.ts';
import { ImportForm } from './forms.tsx';

const refusal: ApiProblem = { code: 'import_refused', message: 'qrels/test.tsv: no such file in /data/notes', hint: 'Correct it, then try again.' } as ApiProblem;

describe('a refusal under a field', () => {
  it('moves focus to the field in the same commit that shows the refusal, so nothing reads the words before focus is there', async () => {
    render(<ImportForm onImport={() => Promise.resolve(refusal)} />);
    const path = screen.getByLabelText('Corpus directory');
    fireEvent.change(path, { target: { value: '/data/notes' } });
    fireEvent.change(screen.getByLabelText('Import as'), { target: { value: 'notes' } });

    // Where focus is at the moment the field is marked invalid: an observer's
    // callback runs right after that commit, before any later effect.
    const seen: (Element | null)[] = [];
    const observer = new MutationObserver(() => {
      if (path.getAttribute('aria-invalid') === 'true') seen.push(document.activeElement);
    });
    observer.observe(path, { attributes: true, attributeFilter: ['aria-invalid'] });
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    await waitFor(() => expect(seen.length).toBeGreaterThan(0));
    observer.disconnect();
    expect(seen[0]).toBe(path);
  });
});
