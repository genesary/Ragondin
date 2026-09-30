// The UI's API types are generated from the API's golden description and never
// written by hand (ADR-C36 § 2). These tests pin what the generator renders for
// each schema form the description uses, that it refuses a form it does not
// know rather than guessing, and that the freshness check `npm run check` runs
// fails on a stale types.ts and passes on a current one.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { renderApiTypes } from '../scripts/api-types.mjs';

const UI_ROOT = fileURLToPath(new URL('..', import.meta.url));
const GOLDEN = fileURLToPath(new URL('../../runtime/ragondin-api/api/v1.json', import.meta.url));
const TYPES = fileURLToPath(new URL('../src/api/types.ts', import.meta.url));

/** A description with the given schemas and paths, and nothing else. */
const description = (schemas: Record<string, unknown>, paths: Record<string, unknown> = {}) => ({
  openapi: '3.0.3',
  info: { title: 't', version: 'v1' },
  paths,
  components: { schemas },
});

/** The body of `export type <name> = …;`, whitespace collapsed. */
function typeOf(source: string, name: string): string {
  const match = source.match(new RegExp(`^export type ${name} = ([\\s\\S]*?);\\n(?:\\n|$)`, 'm'));
  if (match === null) throw new Error(`no type ${name} in:\n${source}`);
  return (match[1] ?? '').replace(/\s+/g, ' ').trim();
}

describe('renderApiTypes', () => {
  it('renders an object with its required and optional properties', () => {
    const out = renderApiTypes(
      description({
        Run: {
          type: 'object',
          required: ['id'],
          properties: { id: { type: 'string' }, note: { type: 'string' } },
        },
      }),
    );
    expect(typeOf(out, 'Run')).toBe('{ id: string; note?: string; }');
  });

  it('renders scalars, arrays, maps, references and nullability', () => {
    const out = renderApiTypes(
      description({
        A: {
          type: 'object',
          required: ['n', 'i', 'b', 'list', 'map', 'ref', 'maybe'],
          properties: {
            n: { type: 'number', format: 'double' },
            i: { type: 'integer', format: 'uint16', minimum: 0, maximum: 65535 },
            b: { type: 'boolean' },
            list: { type: 'array', items: { $ref: '#/components/schemas/B' } },
            map: { type: 'object', additionalProperties: { type: 'number' } },
            ref: { allOf: [{ $ref: '#/components/schemas/B' }] },
            maybe: { type: 'string', nullable: true },
          },
        },
        B: { type: 'object', required: [], properties: {} },
      }),
    );
    expect(typeOf(out, 'A')).toBe(
      '{ n: number; i: number; b: boolean; list: B[]; map: Record<string, number>; ref: B; maybe: string | null; }',
    );
    expect(typeOf(out, 'B')).toBe('Record<string, never>');
  });

  it('renders an enum, a oneOf of single-value enums and an anyOf with null as unions', () => {
    const out = renderApiTypes(
      description({
        Code: { type: 'string', enum: ['run_exists', 'run_unreadable'] },
        Kind: {
          oneOf: [
            { type: 'string', enum: ['query'] },
            { type: 'string', enum: ['chunks'] },
          ],
        },
        Maybe: { anyOf: [{ $ref: '#/components/schemas/Code' }, { enum: [null], nullable: true }] },
        Value: {
          anyOf: [{ type: 'boolean' }, { type: 'array', items: { $ref: '#/components/schemas/Value' } }],
        },
      }),
    );
    expect(typeOf(out, 'Code')).toBe("'run_exists' | 'run_unreadable'");
    expect(typeOf(out, 'Kind')).toBe("'query' | 'chunks'");
    expect(typeOf(out, 'Maybe')).toBe('Code | null');
    expect(typeOf(out, 'Value')).toBe('boolean | Value[]');
  });

  it('parenthesises a union an array holds', () => {
    const out = renderApiTypes(
      description({ L: { type: 'array', items: { anyOf: [{ type: 'string' }, { type: 'boolean' }] } } }),
    );
    expect(typeOf(out, 'L')).toBe('(string | boolean)[]');
  });

  it('carries each description as a doc comment, with a comment terminator defused', () => {
    const out = renderApiTypes(
      description({
        R: {
          description: 'One run.\nWhole */ even this.',
          type: 'object',
          required: ['id'],
          properties: { id: { description: 'Its id.', type: 'string' } },
        },
      }),
    );
    expect(out).toContain('/**\n * One run.\n * Whole *\\/ even this.\n */\nexport type R');
    expect(out).toContain('  /** Its id. */\n  id: string;');
  });

  it('quotes a property name that is not an identifier', () => {
    const out = renderApiTypes(
      description({ H: { type: 'object', required: ['x-build'], properties: { 'x-build': { type: 'string' } } } }),
    );
    expect(typeOf(out, 'H')).toBe("{ 'x-build': string; }");
  });

  it('renders every path: its methods, path parameters, request body and success response', () => {
    const problem = { content: { 'application/problem+json': { schema: { $ref: '#/components/schemas/P' } } } };
    const out = renderApiTypes(
      description(
        { P: { type: 'object', required: [], properties: {} }, R: { type: 'string' } },
        {
          '/runs/{id}': {
            get: {
              parameters: [{ in: 'path', name: 'id', required: true, schema: { type: 'string' } }],
              responses: {
                200: { content: { 'application/json': { schema: { $ref: '#/components/schemas/R' } } } },
                default: problem,
              },
            },
          },
          '/runs': {
            post: {
              parameters: [],
              requestBody: { content: { 'application/json': { schema: { $ref: '#/components/schemas/R' } } } },
              responses: {
                202: { content: { 'application/json': { schema: { $ref: '#/components/schemas/R' } } } },
                default: problem,
              },
            },
          },
        },
      ),
    );
    expect(typeOf(out, 'Paths')).toBe(
      "{ '/runs/{id}': { get: { params: { id: string; }; response: R; }; }; '/runs': { post: { params: Record<string, never>; body: R; response: R; }; }; }",
    );
  });

  it.each([
    ['a schema form it does not know', description({ X: { type: 'object', patternProperties: {} } }), /patternProperties/],
    ['a reference outside the schemas', description({ X: { $ref: 'other.json#/X' } }), /other\.json/],
    [
      'a parameter that is not in the path',
      description({}, { '/x': { get: { parameters: [{ in: 'query', name: 'q', schema: { type: 'string' } }], responses: {} } } }),
      /query/,
    ],
    ['a path with no success response', description({}, { '/x': { get: { parameters: [], responses: {} } } }), /success/],
  ])('refuses %s rather than guessing', (_, input, message) => {
    expect(() => renderApiTypes(input)).toThrow(message);
  });
});

describe('src/api/types.ts', () => {
  it('is what the golden description generates (just gen-ui-types regenerates it)', () => {
    expect(readFileSync(TYPES, 'utf8')).toBe(renderApiTypes(JSON.parse(readFileSync(GOLDEN, 'utf8'))));
  });
});

describe('the freshness check (scripts/check-api-types.mjs)', () => {
  const run = (...args: string[]) => {
    try {
      const out = execFileSync('node', ['scripts/check-api-types.mjs', ...args], {
        cwd: UI_ROOT,
        encoding: 'utf8',
        stdio: 'pipe',
      });
      return { code: 0, out };
    } catch (e) {
      const err = e as { status: number; stdout: string; stderr: string };
      return { code: err.status, out: err.stdout + err.stderr };
    }
  };

  const fixture = () => {
    const dir = mkdtempSync(join(tmpdir(), 'ragondin-api-types-'));
    const golden = JSON.parse(readFileSync(GOLDEN, 'utf8'));
    const descriptionPath = join(dir, 'v1.json');
    const typesPath = join(dir, 'types.ts');
    writeFileSync(descriptionPath, JSON.stringify(golden));
    writeFileSync(typesPath, renderApiTypes(golden));
    return { golden, descriptionPath, typesPath };
  };

  it('passes when the types are what the description generates', () => {
    const { descriptionPath, typesPath } = fixture();
    expect(run(descriptionPath, typesPath).code).toBe(0);
  });

  it('fails, naming the recipe, when the description changed and the types were not regenerated', () => {
    const { golden, descriptionPath, typesPath } = fixture();
    golden.components.schemas.Workspace.properties.added = { type: 'string' };
    writeFileSync(descriptionPath, JSON.stringify(golden));
    const result = run(descriptionPath, typesPath);
    expect(result.code).toBe(1);
    expect(result.out).toMatch(/just gen-ui-types/);
  });

  it('fails on a hand edit of the types', () => {
    const { descriptionPath, typesPath } = fixture();
    writeFileSync(typesPath, readFileSync(typesPath, 'utf8').replace('path: string;', 'path: string | number;'));
    expect(run(descriptionPath, typesPath).code).toBe(1);
  });

  it('checks the committed files when given no argument', () => {
    expect(run().code).toBe(0);
  });

  it('is part of `npm run check`, and so of CI and `just check`', () => {
    const scripts = JSON.parse(readFileSync(join(UI_ROOT, 'package.json'), 'utf8')).scripts as Record<string, string>;
    expect(scripts['types:check']).toBe('node scripts/check-api-types.mjs');
    expect(scripts['check']?.split(' && ')).toContain('npm run types:check');
  });

  it('is regenerated by `just gen-ui-types`', () => {
    const justfile = readFileSync(join(UI_ROOT, '../justfile'), 'utf8');
    expect(justfile).toMatch(/^gen-ui-types:.*\n\s+cd ui && node scripts\/gen-api-types\.mjs$/m);
  });
});
