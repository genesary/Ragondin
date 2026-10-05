// What a node takes: the parameters `GET /workspace` serves for its family
// and `impl:` name — the composition root's declaration, never a table of
// the browser's own — the values written when it is placed, and the
// required keys it lacks. ARCHITECTURE.md § The editor.
import type { Capabilities, Parameter, ParameterValue, ServiceStatus } from '../api/types.ts';

/**
 * The parameters a node of `component` under `impl` takes, with `params` as
 * it holds them: a name bound in the family takes the family's `bound` list;
 * a carried name takes its own, plus what its choice's key picks — the case
 * naming the key's value, else, for any other non-empty text, the case with
 * no value. Null for a name neither, which the capabilities say nothing about.
 */
export function parametersOf(
  capabilities: Capabilities,
  services: readonly ServiceStatus[],
  component: string,
  impl: string,
  params: Readonly<Record<string, ParameterValue>>,
): Parameter[] | null {
  const family = capabilities.families.find((f) => f.family === component);
  if (family === undefined) return null;
  if (services.some((s) => s.family === component && s.name === impl)) return family.bound;
  const own = family.parameters.find((p) => p.name === impl);
  if (own === undefined) return null;
  if (own.choice === null) return own.parameters;
  const chosen = params[own.choice.key];
  const value = chosen?.kind === 'string' && chosen.value !== '' ? chosen.value : null;
  if (value === null) return own.parameters;
  const cases = own.choice.cases;
  const picked = cases.find((c) => c.value === value) ?? cases.find((c) => c.value === null);
  return picked === undefined ? own.parameters : [...own.parameters, ...picked.parameters];
}

/**
 * What a placed node is written with: each required key's starting value,
 * where the composition root gives it one. An ordinary value in the document,
 * hashed as any other; a key with none is left out — no placeholder is ever
 * written, and the key is marked missing instead.
 */
export function startingParams(parameters: readonly Parameter[]): Record<string, ParameterValue> {
  const out: Record<string, ParameterValue> = {};
  for (const p of parameters) if (p.required && p.start !== null) out[p.name] = p.start;
  return out;
}

/** The required keys `params` does not set, in the order they are served: what the server refuses to store or run. */
export function missingRequired(parameters: readonly Parameter[], params: Readonly<Record<string, ParameterValue>>): string[] {
  return parameters.filter((p) => p.required && params[p.name] === undefined).map((p) => p.name);
}
