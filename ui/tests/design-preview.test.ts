import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { get } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build, createServer } from 'vite';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const UI = fileURLToPath(new URL('..', import.meta.url));

/** Every file under `root`, as a path relative to it. */
function list(root: string, dir = root): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? list(root, path) : [relative(root, path)];
  });
}

describe('the preview page is absent from the production build', () => {
  // The same build `npm run build` runs, with the repository's configuration,
  // written to a scratch directory so the test neither needs nor disturbs
  // ui/dist/.
  let out = '';
  let files: string[] = [];
  beforeAll(async () => {
    out = mkdtempSync(join(tmpdir(), 'ui-dist-'));
    await build({ root: UI, logLevel: 'silent', build: { outDir: out, emptyOutDir: true } });
    files = list(out);
  }, 60_000);
  afterAll(() => rmSync(out, { recursive: true, force: true }));

  it('builds the application', () => {
    expect(files).toContain('index.html');
    expect(files.some((f) => f.endsWith('.js'))).toBe(true);
  });

  it('lists no preview file, and no built file carries the preview page', () => {
    expect(files.filter((f) => /preview/i.test(f))).toEqual([]);
    for (const f of files.filter((name) => /\.(html|js|css)$/.test(name))) {
      expect(readFileSync(join(out, f), 'utf8'), f).not.toContain('rg-preview');
    }
  });

  it('bundles the fonts it serves, from this origin', () => {
    expect(files.filter((f) => f.endsWith('.woff2')).length).toBe(9);
  });
});

describe('the preview page on the dev server', () => {
  it('is an entry under ui/design/preview/ that mounts the preview', () => {
    const html = readFileSync(join(UI, 'design/preview/index.html'), 'utf8');
    expect(html).toMatch(/<script type="module" src="\.\/main\.tsx"><\/script>/);
    expect(existsSync(join(UI, 'design/preview/main.tsx'))).toBe(true);
  });

  it('is served by `npm run dev` at /design/preview/', async () => {
    const server = await createServer({ root: UI, logLevel: 'silent', server: { port: 0, host: '127.0.0.1' } });
    await server.listen();
    try {
      const { port } = server.httpServer?.address() as AddressInfo;
      const body = await new Promise<{ status: number; text: string }>((resolve, reject) => {
        get({ host: '127.0.0.1', port, path: '/design/preview/' }, (res) => {
          let text = '';
          res.on('data', (chunk) => (text += chunk));
          res.on('end', () => resolve({ status: res.statusCode ?? 0, text }));
        }).on('error', reject);
      });
      expect(body.status).toBe(200);
      // Not the application's page, which the dev server's fallback would also answer with.
      expect(body.text).toContain('<title>Design system preview</title>');
    } finally {
      await server.close();
    }
  }, 30_000);
});
