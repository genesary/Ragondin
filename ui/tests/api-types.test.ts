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

/** A success response, for the paths a refusal test needs otherwise valid. */
const OK = { 200: { content: { 'application/json': { schema: { type: 'string' } } } } } as const;

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

describe('renderApiTypes, a closed object', () => {
  // `additionalProperties: false` is what a request body that refuses an
  // unknown field states; a TypeScript object type is closed to literals
  // already, so the type is the one the same properties give without it.
  it('renders properties with `additionalProperties: false` as it renders them without', () => {
    const properties = { name: { type: 'string' }, note: { type: 'string', nullable: true } };
    const open = renderApiTypes(description({ X: { type: 'object', required: ['name'], properties } }));
    const closed = renderApiTypes(
      description({ X: { type: 'object', required: ['name'], properties, additionalProperties: false } }),
    );
    expect(typeOf(closed, 'X')).toBe(typeOf(open, 'X'));
    expect(typeOf(closed, 'X')).toBe('{ name: string; note?: string | null; }');
  });

  it.each([
    ['`additionalProperties: true`', { type: 'object', properties: {}, additionalProperties: true }],
    ['`additionalProperties: false` with no properties', { type: 'object', additionalProperties: false }],
  ])('still refuses %s', (_, schema) => {
    expect(() => renderApiTypes(description({ X: schema }))).toThrow();
  });
});

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
    expect(typeOf(out, 'Code')).toBe('"run_exists" | "run_unreadable"');
    expect(typeOf(out, 'Kind')).toBe('"query" | "chunks"');
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
    expect(typeOf(out, 'H')).toBe('{ "x-build": string; }');
  });

  it('escapes an enum value or a property name holding a newline or a backslash, as JSON does', () => {
    const out = renderApiTypes(
      description({
        E: { type: 'string', enum: ['a\nb', 'a\\b'] },
        O: { type: 'object', required: ['a\nb', 'a\\b'], properties: { 'a\nb': { type: 'string' }, 'a\\b': { type: 'string' } } },
      }),
    );
    expect(typeOf(out, 'E')).toBe(String.raw`"a\nb" | "a\\b"`);
    expect(typeOf(out, 'O')).toBe(String.raw`{ "a\nb": string; "a\\b": string; }`);
  });

  it('lists the operations the description declares empty, for the client to accept an empty body from', () => {
    const out = renderApiTypes(
      description({}, { '/jobs/{id}': { delete: { parameters: [{ in: 'path', name: 'id', required: true, schema: { type: 'string' } }], responses: { 204: { description: 'Gone.' } } } } }),
    );
    expect(out).toContain('export const EMPTY_ANSWERS: readonly string[] = ["DELETE /jobs/{id}"];');
    expect(renderApiTypes(description({}))).toContain('export const EMPTY_ANSWERS: readonly string[] = [];');
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
      '{ "/runs/{id}": { get: { params: { id: string; }; response: R; }; }; "/runs": { post: { params: Record<string, never>; body: R; response: R; }; }; }',
    );
  });

  // An event stream is read through `EventSource`, never through the client:
  // its response is `never`, so no client call can claim to read it, and it
  // is not an empty answer. Its events are typed by the schema it names,
  // which is rendered as every schema is.
  it('renders an event stream’s response as never, not as an empty answer', () => {
    const out = renderApiTypes(
      description(
        { E: { type: 'string' } },
        { '/jobs/events': { get: { parameters: [], responses: { 200: { content: { 'text/event-stream': { schema: { $ref: '#/components/schemas/E' } } } } } } } },
      ),
    );
    expect(typeOf(out, 'Paths')).toBe('{ "/jobs/events": { get: { params: Record<string, never>; response: never; }; }; }');
    expect(out).toContain('export const EMPTY_ANSWERS: readonly string[] = [];');
    expect(typeOf(out, 'E')).toBe('string');
  });

  it('renders query and header parameters beside the path’s, each optional unless required', () => {
    const R = { 200: { content: { 'application/json': { schema: { $ref: '#/components/schemas/R' } } } } };
    const string = { type: 'string' };
    const out = renderApiTypes(
      description(
        { R: { type: 'string' } },
        {
          '/q': { get: { parameters: [{ in: 'query', name: 'missing_gold_at', required: false, schema: { type: 'integer', minimum: 1 } }], responses: R } },
          '/h': {
            get: {
              parameters: [
                { in: 'header', name: 'If-Match', required: false, description: 'The etag read.', schema: string },
                { in: 'header', name: 'X-Required', required: true, schema: string },
              ],
              responses: R,
            },
          },
          '/both/{id}': {
            put: {
              parameters: [
                { in: 'path', name: 'id', required: true, schema: string },
                { in: 'query', name: 'n', required: true, schema: { type: 'integer' } },
                { in: 'header', name: 'If-None-Match', required: false, schema: string },
              ],
              requestBody: { content: { 'application/json': { schema: { $ref: '#/components/schemas/R' } } } },
              responses: R,
            },
          },
        },
      ),
    );
    expect(typeOf(out, 'Paths')).toBe(
      '{ "/q": { get: { params: Record<string, never>; query: { missing_gold_at?: number; }; response: R; }; }; ' +
        '"/h": { get: { params: Record<string, never>; headers: { /** The etag read. */ "If-Match"?: string; "X-Required": string; }; response: R; }; }; ' +
        '"/both/{id}": { put: { params: { id: string; }; query: { n: number; }; headers: { "If-None-Match"?: string; }; body: R; response: R; }; }; }',
    );
  });

  it.each([
    ['a schema form it does not know', description({ X: { type: 'object', patternProperties: {} } }), /patternProperties/],
    ['a reference outside the schemas', description({ X: { $ref: 'other.json#/X' } }), /other\.json/],
    [
      'a parameter in a cookie, naming the operation',
      description({}, { '/x': { get: { parameters: [{ in: 'cookie', name: 'c', required: false, schema: { type: 'string' } }], responses: OK } } }),
      /GET \/x.*cookie/,
    ],
    [
      'a parameter located in an inherited key (`constructor`)',
      description({}, { '/x': { get: { parameters: [{ in: 'constructor', name: 'c', required: true, schema: { type: 'string' } }], responses: OK } } }),
      /GET \/x.*constructor/,
    ],
    [
      'a parameter located in an inherited key (`__proto__`)',
      description({}, { '/x': { get: { parameters: [{ in: '__proto__', name: 'c', required: true, schema: { type: 'string' } }], responses: OK } } }),
      /GET \/x.*__proto__/,
    ],
    [
      'two query parameters of one name',
      description({}, {
        '/x': {
          get: {
            parameters: [
              { in: 'query', name: 'q', required: false, schema: { type: 'string' } },
              { in: 'query', name: 'q', required: false, schema: { type: 'integer' } },
            ],
            responses: OK,
          },
        },
      }),
      /GET \/x.*`q`.*twice/,
    ],
    [
      'two headers of one name',
      description({}, {
        '/x': {
          get: {
            parameters: [
              { in: 'header', name: 'If-Match', required: false, schema: { type: 'string' } },
              { in: 'header', name: 'If-Match', required: false, schema: { type: 'string' } },
            ],
            responses: OK,
          },
        },
      }),
      /GET \/x.*`If-Match`.*twice/,
    ],
    [
      'two headers whose names differ only in case, which HTTP reads as one',
      description({}, {
        '/x': {
          get: {
            parameters: [
              { in: 'header', name: 'If-Match', required: false, schema: { type: 'string' } },
              { in: 'header', name: 'if-match', required: false, schema: { type: 'string' } },
            ],
            responses: OK,
          },
        },
      }),
      /GET \/x.*`if-match`.*twice/,
    ],
    [
      'a parameter in no location it knows',
      description({}, { '/x': { get: { parameters: [{ in: 'body', name: 'b', required: true, schema: { type: 'string' } }], responses: OK } } }),
      /GET \/x.*body/,
    ],
    ['a path with no success response', description({}, { '/x': { get: { parameters: [], responses: {} } } }), /success/],
    ['a second success response', description({}, { '/x': { get: { responses: { ...OK, 201: OK[200] } } } }), /second success/],
    ['an object with neither properties nor additionalProperties', description({ X: { type: 'object' } }), /neither/],
    ['properties on a type that is not an object', description({ X: { type: 'string', properties: {} } }), /properties/],
    ['a required name that is not a property', description({ X: { type: 'object', required: ['gone'], properties: {} } }), /gone/],
    ['an operation keyword it does not check (security)', description({}, { '/x': { get: { security: [], responses: OK } } }), /security/],
    ['an operation keyword it does not check (deprecated)', description({}, { '/x': { get: { deprecated: true, responses: OK } } }), /deprecated/],
    [
      'an optional request body',
      description({}, { '/x': { post: { requestBody: { required: false, content: { 'application/json': { schema: { type: 'string' } } } }, responses: OK } } }),
      /optional request body/,
    ],
    ['a path-level parameters list', description({}, { '/x': { parameters: [], get: { responses: OK } } }), /parameters/],
    ['a path-level summary', description({}, { '/x': { summary: 's', get: { responses: OK } } }), /summary/],
    ['an OpenAPI version other than 3.0.x', { ...description({}), openapi: '3.1.0' }, /3\.1\.0/],
    ['a schema name that is not an identifier', description({ 'Run-Detail': { type: 'string' } }), /Run-Detail/],
    ['a schema named as the generator’s own output', description({ Paths: { type: 'string' } }), /Paths/],
    ['a reference to a schema that does not exist', description({ X: { $ref: '#/components/schemas/Gone' } }), /Gone/],
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
