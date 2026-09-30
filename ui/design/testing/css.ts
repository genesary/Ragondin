// A minimal CSS reader for the design system's tests: enough to list the
// rules of a stylesheet this tree writes, in order, with the at-rule each sits
// in. It is not a general parser and need not be: a stylesheet it cannot read
// fails the test that reads it. Test-only; nothing in the bundle imports it.

export type Rule = {
  /** The enclosing at-rule's prelude, e.g. `@media (prefers-color-scheme: dark)`, or null at top level. */
  atRule: string | null;
  /** The selector list, whitespace collapsed. */
  selector: string;
  /** Declarations in source order; a custom property keeps its `--` prefix. */
  declarations: Map<string, string>;
};

export function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

function parseDeclarations(body: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const part of body.split(';')) {
    const colon = part.indexOf(':');
    if (colon < 0) continue;
    const name = part.slice(0, colon).trim();
    if (name !== '') out.set(name, part.slice(colon + 1).trim());
  }
  return out;
}

function walk(source: string, atRule: string | null, rules: Rule[]): void {
  let k = 0;
  while (k < source.length) {
    const open = source.indexOf('{', k);
    if (open < 0) return;
    // A statement at-rule (`@import …;`) before the block is not part of its prelude.
    const prelude = source.slice(k, open).split(';').pop()!.trim().replace(/\s+/g, ' ');
    let depth = 1;
    let j = open + 1;
    while (j < source.length && depth > 0) {
      if (source[j] === '{') depth++;
      else if (source[j] === '}') depth--;
      j++;
    }
    const body = source.slice(open + 1, j - 1);
    if (prelude.startsWith('@')) {
      if (/^@(media|supports|layer)\b/.test(prelude)) walk(body, prelude, rules);
    } else {
      rules.push({ atRule, selector: prelude, declarations: parseDeclarations(body) });
    }
    k = j;
  }
}

/** Every style rule of `css`, in source order. */
export function parseRules(css: string): Rule[] {
  const rules: Rule[] = [];
  walk(stripComments(css), null, rules);
  return rules;
}

/** The members of a selector list. */
export function selectors(rule: Rule): string[] {
  return rule.selector.split(',').map((s) => s.trim());
}

/** The rules whose selector list has `selector` as one of its members. */
export function rulesFor(css: string, selector: string): Rule[] {
  return parseRules(css).filter((rule) => selectors(rule).includes(selector));
}

/** The value `property` takes in the first rule naming `selector`, or undefined. */
export function declared(css: string, selector: string, property: string): string | undefined {
  for (const rule of rulesFor(css, selector)) {
    const value = rule.declarations.get(property);
    if (value !== undefined) return value;
  }
  return undefined;
}
