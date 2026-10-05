import { describe, expect, it } from 'vitest';
import type { Capabilities, Parameter, ServiceStatus } from '../api/types.ts';
import { int, str } from '../parameters.ts';
import { missingRequired, parametersOf, startingParams } from './parameters.ts';

const p = (name: string, required: boolean, start: Parameter['start'] = null, kind: Parameter['kind'] = 'non_negative_integer'): Parameter => ({
  name,
  kind,
  required,
  description: `What ${name} is for.`,
  start,
});
const TOP_K = p('top_k', true, int('10'));

const CAPS: Capabilities = {
  families: [
    {
      family: 'retriever',
      ports: null,
      not_carried: [],
      parameters: [
        { name: 'bm25', parameters: [TOP_K], choice: null },
        {
          name: 'dense',
          parameters: [TOP_K, p('embedder', true, null, 'string')],
          choice: {
            key: 'embedder',
            cases: [
              { value: 'onnx', parameters: [p('model', true, null, 'string')] },
              { value: null, parameters: [p('served_model', true, null, 'string')] },
            ],
          },
        },
      ],
      bound: [TOP_K],
    },
    {
      family: 'generator',
      ports: null,
      not_carried: [],
      parameters: [],
      bound: [p('served_model', true, null, 'string'), p('template', true, str('{context}'), 'string'), p('seed', false)],
    },
  ],
  remote: true,
};
const QWEN: ServiceStatus = { family: 'generator', name: 'qwen', uri: '127.0.0.1:1', connected: true, identity: null };

const names = (list: Parameter[] | null) => list?.map((x) => x.name) ?? null;

describe('the parameters a node takes', () => {
  it('are its implementation’s, as the capabilities serve them', () => {
    expect(names(parametersOf(CAPS, [], 'retriever', 'bm25', {}))).toEqual(['top_k']);
  });

  it('are a bound name’s family list', () => {
    expect(names(parametersOf(CAPS, [QWEN], 'generator', 'qwen', {}))).toEqual(['served_model', 'template', 'seed']);
  });

  it('add the keys the node’s choice picks: the named case, else the case for any other value', () => {
    expect(names(parametersOf(CAPS, [], 'retriever', 'dense', {}))).toEqual(['top_k', 'embedder']);
    expect(names(parametersOf(CAPS, [], 'retriever', 'dense', { embedder: str('onnx') }))).toEqual(['top_k', 'embedder', 'model']);
    expect(names(parametersOf(CAPS, [], 'retriever', 'dense', { embedder: str('bge') }))).toEqual(['top_k', 'embedder', 'served_model']);
    expect(names(parametersOf(CAPS, [], 'retriever', 'dense', { embedder: str('') }))).toEqual(['top_k', 'embedder']);
  });

  it('are unknown for a name neither carried nor bound', () => {
    expect(parametersOf(CAPS, [], 'retriever', 'nothing', {})).toBeNull();
    expect(parametersOf(CAPS, [], 'fusion', 'rrf', {})).toBeNull();
  });
});

describe('a placed node’s parameters', () => {
  it('are the starting values of its required keys, and nothing for a key with none', () => {
    expect(startingParams(parametersOf(CAPS, [QWEN], 'generator', 'qwen', {}) ?? [])).toEqual({ template: str('{context}') });
  });

  it('give an optional key nothing, even one served with a value', () => {
    expect(startingParams([p('seed', false, int('1'))])).toEqual({});
  });
});

describe('a missing required key', () => {
  it('is each required key the node does not set, in served order', () => {
    const list = parametersOf(CAPS, [QWEN], 'generator', 'qwen', {}) ?? [];
    expect(missingRequired(list, { template: str('{context}') })).toEqual(['served_model']);
    expect(missingRequired(list, { template: str('x'), served_model: str('m') })).toEqual([]);
  });
});
