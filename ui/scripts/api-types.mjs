// Renders the API's golden description, runtime/ragondin-api/api/v1.json, into
// the UI's TypeScript types, ui/src/api/types.ts (ADR-C36 § 2: generated, never
// written by hand). It knows exactly the schema forms the description uses and
// throws on any other, so a change to the description's shape stops the
// generator rather than degrading a type to `unknown`; such a change is raised
// against runtime/ragondin-api, never patched in types.ts. Why this is a script
// here rather than a package is ui/ARCHITECTURE.md § The generated types.

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

/** @param {string} key */
const propertyName = (key) => (IDENTIFIER.test(key) ? key : `'${key.replace(/'/g, "\\'")}'`);

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
 * intersection must parenthesise.
 * @param {string} type
 */
function isUnion(type) {
  let depth = 0;
  for (let i = 0; i < type.length; i += 1) {
    const c = type[i];
    if (c === '(' || c === '{' || c === '<') depth += 1;
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
 * The TypeScript type of one schema.
 * @param {Schema} schema
 * @param {string} where
 * @param {string} indent
 * @returns {string}
 */
function typeOf(schema, where, indent) {
  for (const key of Object.keys(schema)) if (!KNOWN.has(key)) refuse(where, `unknown schema keyword \`${key}\``);
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
    return schema.$ref.slice(REF.length);
  }
  if (Array.isArray(schema.enum)) {
    return join(
      schema.enum.map((/** @type {unknown} */ v) => {
        if (v === null) return 'null';
        if (typeof v === 'string') return `'${v.replace(/'/g, "\\'")}'`;
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
  const properties = /** @type {Record<string, Schema>} */ (schema.properties ?? {});
  const required = new Set(/** @type {string[]} */ (schema.required ?? []));
  const extra = schema.additionalProperties;
  const names = Object.keys(properties);
  if (extra !== undefined && typeof extra !== 'object') refuse(where, '`additionalProperties` that is not a schema');
  if (extra !== undefined && names.length > 0) refuse(where, 'both `properties` and `additionalProperties`');
  if (extra !== undefined) return `Record<string, ${typeOf(extra, `${where}.additionalProperties`, indent)}>`;
  if (names.length === 0) return 'Record<string, never>';
  const inner = `${indent}  `;
  const lines = names.map((name) => {
    const property = properties[name] ?? {};
    const optional = required.has(name) ? '' : '?';
    return `${doc(property.description, inner)}${inner}${propertyName(name)}${optional}: ${typeOf(property, `${where}.${name}`, inner)};`;
  });
  return `{\n${lines.join('\n')}\n${indent}}`;
}

/**
 * The JSON schema of the one success response, or null for one with no body.
 * @param {Record<string, Schema>} responses
 * @param {string} where
 */
function successSchema(responses, where) {
  const code = Object.keys(responses).find((c) => /^2\d\d$/.test(c));
  if (code === undefined) return refuse(where, 'no success response');
  const content = responses[code]?.content;
  if (content === undefined) return null;
  const json = content['application/json'];
  if (json === undefined) return refuse(where, `a success response that is not application/json (${Object.keys(content).join(', ')})`);
  return json.schema;
}

/**
 * @param {Record<string, Record<string, Schema>>} paths
 */
function pathsType(paths) {
  const entries = Object.entries(paths).map(([path, methods]) => {
    const operations = Object.entries(methods).map(([method, op]) => {
      const where = `${method.toUpperCase()} ${path}`;
      const params = (/** @type {Schema[]} */ (op.parameters ?? [])).map((p) => {
        if (p.in !== 'path') refuse(where, `a parameter in \`${p.in}\`, not in the path`);
        return `        ${propertyName(p.name)}: ${typeOf(p.schema ?? {}, `${where} ${p.name}`, '        ')};`;
      });
      const lines = [params.length === 0 ? '      params: Record<string, never>;' : `      params: {\n${params.join('\n')}\n      };`];
      if (op.requestBody !== undefined) {
        const body = op.requestBody.content?.['application/json']?.schema;
        if (body === undefined) refuse(where, 'a request body that is not application/json');
        lines.push(`      body: ${typeOf(body, `${where} body`, '      ')};`);
      }
      const response = successSchema(op.responses ?? {}, where);
      lines.push(`      response: ${response === null ? 'null' : typeOf(response, `${where} response`, '      ')};`);
      return `${doc(op.summary, '    ')}    ${method}: {\n${lines.join('\n')}\n    };`;
    });
    return `  '${path}': {\n${operations.join('\n')}\n  };`;
  });
  return entries.length === 0 ? 'Record<string, never>' : `{\n${entries.join('\n')}\n}`;
}

/**
 * The whole of types.ts for a description.
 * @param {Schema} description
 * @returns {string}
 */
export function renderApiTypes(description) {
  const schemas = /** @type {Record<string, Schema>} */ (description.components?.schemas ?? {});
  const blocks = Object.entries(schemas).map(
    ([name, schema]) => `${doc(schema.description, '')}export type ${name} = ${typeOf(schema, name, '')};`,
  );
  blocks.push(
    `/** Every path under the API's base address: per method, its path parameters, its request body and its success response. */\nexport type Paths = ${pathsType(description.paths ?? {})};`,
  );
  return `${HEADER}\n${blocks.join('\n\n')}\n`;
}
