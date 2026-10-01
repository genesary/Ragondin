// Renders the API's golden description, runtime/ragondin-api/api/v1.json, into
// the UI's TypeScript types, ui/src/api/types.ts (ADR-C36 § 2: generated, never
// written by hand). It knows exactly the forms the description uses and throws
// on any other — a schema keyword, an operation keyword, a path-level key, an
// OpenAPI version — so a change to the description's shape stops the generator
// rather than degrading a type to `unknown` or dropping a constraint. Such a
// change is raised against runtime/ragondin-api, never patched in types.ts.
// Why this is a script here rather than a package is ui/ARCHITECTURE.md § The
// generated types.

/** @typedef {Record<string, any>} Schema */

const REF = '#/components/schemas/';

// The keys a schema may carry that change no type: documentation and numeric
// bounds the server enforces.
const INERT = new Set(['description', 'format', 'minimum', 'maximum']);
const KNOWN = new Set([
  ...INERT,
  '$ref',
  'type',
  'enum',
  'nullable',
  'oneOf',
  'anyOf',
  'allOf',
  'properties',
  'required',
  'additionalProperties',
  'items',
]);
const METHODS = new Set(['get', 'post', 'put', 'patch', 'delete']);
const OPERATION_KEYS = new Set(['summary', 'description', 'parameters', 'requestBody', 'responses']);
const REQUEST_BODY_KEYS = new Set(['description', 'content', 'required']);
const PARAMETER_KEYS = new Set(['in', 'name', 'required', 'description', 'schema']);
// The names this file exports besides the schemas.
const RESERVED = new Set(['Paths', 'EMPTY_ANSWERS']);

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

const HEADER = `// Generated from runtime/ragondin-api/api/v1.json by \`just gen-ui-types\`
// (ui/scripts/gen-api-types.mjs). Do not edit: \`npm run check\` fails when this
// file is not what the description generates (ui/ARCHITECTURE.md § The
// generated types).
`;

/**
 * @param {string} where
 * @param {string} what
 * @returns {never}
 */
function refuse(where, what) {
  throw new Error(`${where}: ${what}. The generator refuses it rather than guessing a type; see ui/ARCHITECTURE.md § The generated types.`);
}

/**
 * Refuses any key of `object` outside `allowed`.
 * @param {Record<string, unknown>} object
 * @param {Set<string>} allowed
 * @param {string} where
 * @param {string} kind
 */
function onlyKnown(object, allowed, where, kind) {
  for (const key of Object.keys(object)) if (!allowed.has(key)) refuse(where, `${kind} keyword \`${key}\` this generator does not check`);
}

/** A string as a TypeScript literal: JSON's escaping is TypeScript's. */
const literal = (/** @type {string} */ value) => JSON.stringify(value);

/** @param {string} key */
const propertyName = (key) => (IDENTIFIER.test(key) ? key : literal(key));

/**
 * A doc comment, indented, or nothing.
 * @param {unknown} text
 * @param {string} indent
 */
function doc(text, indent) {
  if (typeof text !== 'string' || text === '') return '';
  const lines = text.replace(/\*\//g, '*\\/').split('\n');
  if (lines.length === 1) return `${indent}/** ${lines[0]} */\n`;
  return `${indent}/**\n${lines.map((l) => `${indent} *${l === '' ? '' : ` ${l}`}`).join('\n')}\n${indent} */\n`;
}

/**
 * Whether a rendered type is a union at its top level, which an array or an
 * intersection must parenthesise. String literals are skipped whole, so a `|`
 * inside one is not taken for a union.
 * @param {string} type
 */
function isUnion(type) {
  let depth = 0;
  for (let i = 0; i < type.length; i += 1) {
    const c = type[i];
    if (c === '"') {
      for (i += 1; i < type.length && type[i] !== '"'; i += 1) if (type[i] === '\\') i += 1;
    } else if (c === '(' || c === '{' || c === '<') depth += 1;
    else if (c === ')' || c === '}' || c === '>') depth -= 1;
    else if (c === '|' && depth === 0) return true;
  }
  return false;
}

/**
 * @param {string[]} members
 * @param {string} joiner
 */
const join = (members, joiner) => [...new Set(members)].join(joiner);

/**
 * Renders schemas against the set of names the description defines, so a
 * reference to any other is refused.
 * @param {Set<string>} names
 */
function renderer(names) {
  /**
   * The TypeScript type of one schema.
   * @param {Schema} schema
   * @param {string} where
   * @param {string} indent
   * @returns {string}
   */
  function typeOf(schema, where, indent) {
    if (typeof schema !== 'object' || schema === null || Array.isArray(schema)) refuse(where, 'a schema that is not an object');
    onlyKnown(schema, KNOWN, where, 'schema');
    if ((schema.properties !== undefined || schema.required !== undefined || schema.additionalProperties !== undefined) && schema.type !== 'object') {
      refuse(where, '`properties`, `required` or `additionalProperties` on a schema whose type is not `object`');
    }
    const base = baseType(schema, where, indent);
    return schema.nullable === true && base !== 'null' ? `${base} | null` : base;
  }

  /**
   * @param {Schema} schema
   * @param {string} where
   * @param {string} indent
   * @returns {string}
   */
  function baseType(schema, where, indent) {
    if (typeof schema.$ref === 'string') {
      if (!schema.$ref.startsWith(REF)) refuse(where, `a reference outside the description's schemas, \`${schema.$ref}\``);
      const name = schema.$ref.slice(REF.length);
      if (!names.has(name)) refuse(where, `a reference to \`${name}\`, which the description does not define`);
      return name;
    }
    if (Array.isArray(schema.enum)) {
      return join(
        schema.enum.map((/** @type {unknown} */ v) => {
          if (v === null) return 'null';
          if (typeof v === 'string') return literal(v);
          return refuse(where, `an enum value that is not a string, \`${JSON.stringify(v)}\``);
        }),
        ' | ',
      );
    }
    for (const key of ['oneOf', 'anyOf']) {
      if (Array.isArray(schema[key])) {
        return join(
          schema[key].map((/** @type {Schema} */ s, /** @type {number} */ i) => typeOf(s, `${where}.${key}[${i}]`, indent)),
          ' | ',
        );
      }
    }
    if (Array.isArray(schema.allOf)) {
      const members = schema.allOf.map((/** @type {Schema} */ s, /** @type {number} */ i) => {
        const t = typeOf(s, `${where}.allOf[${i}]`, indent);
        return isUnion(t) ? `(${t})` : t;
      });
      return join(members, ' & ');
    }
    switch (schema.type) {
      case 'string':
        return 'string';
      case 'integer':
      case 'number':
        return 'number';
      case 'boolean':
        return 'boolean';
      case 'array': {
        if (typeof schema.items !== 'object' || schema.items === null) refuse(where, 'an array without `items`');
        const item = typeOf(schema.items, `${where}.items`, indent);
        return isUnion(item) ? `(${item})[]` : `${item}[]`;
      }
      case 'object':
        return objectType(schema, where, indent);
      default:
        return refuse(where, `a schema with no type this generator knows (\`${JSON.stringify(schema.type)}\`)`);
    }
  }

  /**
   * @param {Schema} schema
   * @param {string} where
   * @param {string} indent
   */
  function objectType(schema, where, indent) {
    // `additionalProperties: false` beside `properties` closes the object to
    // other members — what a request body refusing an unknown field states.
    // A TypeScript object type is closed to literals already, so it changes
    // no type, and is read as absent. Alone, or as `true`, it is refused below.
    const closed = schema.additionalProperties === false && schema.properties !== undefined;
    const extra = closed ? undefined : schema.additionalProperties;
    if (schema.properties === undefined && extra === undefined) refuse(where, 'an object with neither `properties` nor `additionalProperties`');
    if (extra !== undefined && (typeof extra !== 'object' || extra === null)) refuse(where, '`additionalProperties` that is not a schema');
    if (extra !== undefined && schema.properties !== undefined) refuse(where, 'both `properties` and `additionalProperties`');
    if (extra !== undefined) return `Record<string, ${typeOf(extra, `${where}.additionalProperties`, indent)}>`;
    const properties = /** @type {Record<string, Schema>} */ (schema.properties);
    const required = /** @type {string[]} */ (schema.required ?? []);
    for (const name of required) if (!(name in properties)) refuse(where, `\`required\` names \`${name}\`, which is not among its properties`);
    const names = Object.keys(properties);
    if (names.length === 0) return 'Record<string, never>';
    const inner = `${indent}  `;
    const lines = names.map((name) => {
      const property = properties[name] ?? {};
      const optional = required.includes(name) ? '' : '?';
      return `${doc(property.description, inner)}${inner}${propertyName(name)}${optional}: ${typeOf(property, `${where}.${name}`, inner)};`;
    });
    return `{\n${lines.join('\n')}\n${indent}}`;
  }

  return typeOf;
}

/**
 * The JSON schema of the one success response, or null for one with no body.
 * @param {Record<string, Schema>} responses
 * @param {string} where
 */
function successSchema(responses, where) {
  const codes = Object.keys(responses).filter((c) => /^2\d\d$/.test(c));
  if (codes.length === 0) return refuse(where, 'no success response');
  if (codes.length > 1) refuse(where, `a second success response (${codes.join(', ')}), which one type cannot tell apart`);
  const content = responses[codes[0] ?? '']?.content;
  if (content === undefined) return null;
  const json = content['application/json'];
  if (json === undefined) return refuse(where, `a success response that is not application/json (${Object.keys(content).join(', ')})`);
  return json.schema;
}

/**
 * `Paths`, and the operations whose success response has no body.
 * @param {Record<string, Record<string, Schema>>} paths
 * @param {(schema: Schema, where: string, indent: string) => string} typeOf
 */
function pathsType(paths, typeOf) {
  /** @type {string[]} */
  const empty = [];
  const entries = Object.entries(paths).map(([path, methods]) => {
    for (const key of Object.keys(methods)) if (!METHODS.has(key)) refuse(path, `a path-level \`${key}\`, which this generator does not read`);
    const operations = Object.entries(methods).map(([method, op]) => {
      const where = `${method.toUpperCase()} ${path}`;
      onlyKnown(op, OPERATION_KEYS, where, 'operation');
      // A Map, so a location named after an inherited key (`constructor`,
      // `__proto__`) is unknown rather than an object's own member.
      /** @type {Map<unknown, string[]>} */
      const byPlace = new Map([
        ['path', []],
        ['query', []],
        ['header', []],
      ]);
      /** @type {Set<string>} */
      const seen = new Set();
      for (const p of /** @type {Schema[]} */ (op.parameters ?? [])) {
        onlyKnown(p, PARAMETER_KEYS, where, 'parameter');
        const place = byPlace.get(p.in);
        if (place === undefined) return refuse(where, `a parameter in \`${String(p.in)}\`, which is not the path, the query string or a header`);
        // One name per location: a second would be a second member of one
        // type. A header's name is case-insensitive in HTTP, so two that
        // differ only in case are one header declared twice.
        const key = `${p.in}\n${p.in === 'header' ? String(p.name).toLowerCase() : p.name}`;
        if (seen.has(key)) refuse(where, `the ${p.in} parameter \`${p.name}\` declared twice`);
        seen.add(key);
        // A path parameter is always required; a query or header one is
        // optional unless the description says otherwise.
        const optional = p.in !== 'path' && p.required !== true ? '?' : '';
        const comment = p.in === 'path' ? '' : doc(p.description, '        ');
        place.push(`${comment}        ${propertyName(p.name)}${optional}: ${typeOf(p.schema ?? {}, `${where} ${p.name}`, '        ')};`);
      }
      const block = (/** @type {string} */ key, /** @type {string[]} */ members) => `      ${key}: {\n${members.join('\n')}\n      };`;
      const inPath = byPlace.get('path') ?? [];
      const inQuery = byPlace.get('query') ?? [];
      const inHeader = byPlace.get('header') ?? [];
      const lines = [inPath.length === 0 ? '      params: Record<string, never>;' : block('params', inPath)];
      if (inQuery.length > 0) lines.push(block('query', inQuery));
      if (inHeader.length > 0) lines.push(block('headers', inHeader));
      if (op.requestBody !== undefined) {
        onlyKnown(op.requestBody, REQUEST_BODY_KEYS, where, 'request body');
        if (op.requestBody.required === false) refuse(where, 'an optional request body, which the client always sends');
        const body = op.requestBody.content?.['application/json']?.schema;
        if (body === undefined) refuse(where, 'a request body that is not application/json');
        lines.push(`      body: ${typeOf(body, `${where} body`, '      ')};`);
      }
      const response = successSchema(op.responses ?? {}, where);
      if (response === null) empty.push(`${method.toUpperCase()} ${path}`);
      lines.push(`      response: ${response === null ? 'null' : typeOf(response, `${where} response`, '      ')};`);
      return `${doc(op.summary, '    ')}    ${method}: {\n${lines.join('\n')}\n    };`;
    });
    return `  ${literal(path)}: {\n${operations.join('\n')}\n  };`;
  });
  return { paths: entries.length === 0 ? 'Record<string, never>' : `{\n${entries.join('\n')}\n}`, empty };
}

/**
 * The whole of types.ts for a description.
 * @param {Schema} description
 * @returns {string}
 */
export function renderApiTypes(description) {
  if (typeof description.openapi !== 'string' || !/^3\.0\.\d+$/.test(description.openapi)) {
    refuse('openapi', `version \`${String(description.openapi)}\`; this generator reads 3.0.x`);
  }
  const schemas = /** @type {Record<string, Schema>} */ (description.components?.schemas ?? {});
  for (const name of Object.keys(schemas)) {
    if (!IDENTIFIER.test(name)) refuse(name, 'a schema name that is not a TypeScript identifier');
    if (RESERVED.has(name)) refuse(name, 'a schema name this file already exports');
  }
  const typeOf = renderer(new Set(Object.keys(schemas)));
  const blocks = Object.entries(schemas).map(([name, schema]) => `${doc(schema.description, '')}export type ${name} = ${typeOf(schema, name, '')};`);
  const { paths, empty } = pathsType(description.paths ?? {}, typeOf);
  blocks.push(`/** Every path under the API's base address: per method, its path parameters, its request body and its success response. */\nexport type Paths = ${paths};`);
  blocks.push(
    `/** The operations whose success response has no body: the client accepts an empty answer from these, and from a 204. */\nexport const EMPTY_ANSWERS: readonly string[] = [${empty.map(literal).join(', ')}];`,
  );
  return `${HEADER}\n${blocks.join('\n\n')}\n`;
}
