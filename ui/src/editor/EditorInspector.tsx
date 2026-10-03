import { useEffect, useId, useState, type KeyboardEvent } from 'react';
import { Button, Checkbox, Input, Inspector, familyOfComponent } from '../../design/index.ts';
import type { ParameterValue } from '../api/types.ts';
import { PORT_LABEL } from '../canvas/Port.tsx';
import { freshId, type WireDocument, type WireNode } from './document.ts';
import type { NodePorts } from './ports.ts';
import type { EditorAction } from './store.ts';

/** What the server said about this node: its words, and the input port the edge it named enters, if it named one. */
export type NodeVerdict = { message: string; port: number | null } | null;

export type EditorInspectorProps = {
  doc: WireDocument;
  node: WireNode;
  ports: NodePorts;
  verdict: NodeVerdict;
  /** What the slot says when the server names nothing here: checking, clear, stopped elsewhere, or no verdict. */
  quiet: string;
  dispatch: (action: EditorAction) => void;
  /** The node was renamed: the selection follows it. */
  onRenamed: (id: string) => void;
};

const show = (value: ParameterValue): string => (Array.isArray(value) ? value.map(show).join(', ') : String(value));

// A typed value read back as the type it had (ADR-C22's flat grammar): a
// number stays a number, a flag a flag, text text, and a list's items take
// the type of its first, or are read as a new value's are.
// `1`, `1.5`, `.5`, `1.`, `1e3`, each optionally signed: the forms a person types for a number.
const NUMBER = /^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/;
function infer(text: string): ParameterValue {
  const t = text.trim();
  if (NUMBER.test(t)) return Number(t);
  if (t === 'true' || t === 'false') return t === 'true';
  if (t.includes(',')) return t.split(',').map((item) => infer(item));
  return text;
}
function readAs(old: ParameterValue, text: string): ParameterValue | null {
  if (Array.isArray(old)) {
    const items = text.split(',').map((item) => item.trim()).filter((item) => item !== '');
    const first = old[0];
    return items.map((item) => (first === undefined || Array.isArray(first) ? infer(item) : (readAs(first, item) ?? item)));
  }
  if (typeof old === 'number') return NUMBER.test(text.trim()) ? Number(text.trim()) : null;
  return text;
}

function Parameter({ prefix, name, value, onSet, onRemove }: { prefix: string; name: string; value: ParameterValue; onSet: (v: ParameterValue) => void; onRemove: () => void }) {
  const [text, setText] = useState(show(value));
  const [error, setError] = useState<string | undefined>(undefined);
  useEffect(() => setText(show(value)), [value]);
  const commit = () => {
    const read = readAs(value, text);
    if (read === null) {
      setError(`${name} is a number: "${text}" is not one.`);
      return;
    }
    setError(undefined);
    onSet(read);
  };
  const remove = (
    <Button kind="quiet" size="s" icon="close" onClick={onRemove}>
      <span className="rg-visually-hidden">Remove {name}</span>
    </Button>
  );
  if (typeof value === 'boolean') {
    return (
      <div className="rg-editor-inspector__param">
        <Checkbox label={name} checked={value} onChange={onSet} />
        {remove}
      </div>
    );
  }
  return (
    <div className="rg-editor-inspector__param">
      <Input
        id={`${prefix}-param-${name}`}
        label={name}
        mono
        numeric={typeof value === 'number'}
        value={text}
        // The line is always there, so an error replaces it rather than pushing the fields below down.
        help={Array.isArray(value) ? 'A list: items separated by commas.' : ' '}
        {...(error === undefined ? {} : { error })}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
        }}
      />
      {remove}
    </div>
  );
}

/**
 * The inspector in write mode: the node's family, `impl:` name and id — the
 * id editable, a taken one refused here before the server would — its input
 * ports with what feeds each, and its parameters under the flat grammar of
 * ADR-C22, one field each in key order, the order the wire schema keeps them
 * in, so nodes of one family read alike. What the server said about the node
 * sits in a slot of its own at the top, always there, so a verdict arriving
 * moves no field under the pointer; an edge it named is said again on that
 * port's row.
 */
export function EditorInspector({ doc, node, ports, verdict, quiet, dispatch, onRenamed }: EditorInspectorProps) {
  const prefix = useId();
  const [id, setId] = useState(node.id);
  const [idError, setIdError] = useState<string | undefined>(undefined);
  const [key, setKey] = useState('');
  const [value, setValue] = useState('');
  useEffect(() => {
    setId(node.id);
    setIdError(undefined);
  }, [node.id]);

  const rename = () => {
    const to = id.trim();
    if (to === node.id) return setIdError(undefined);
    if (to === '') return setIdError('A node needs an id.');
    if (freshId(doc, to) !== to) return setIdError(`\`${to}\` is taken: a node, an input or an edge already names it.`);
    setIdError(undefined);
    dispatch({ type: 'rename', node: node.id, to });
    onRenamed(to);
  };
  const add = () => {
    const name = key.trim();
    if (name === '' || value.trim() === '') return;
    dispatch({ type: 'setParam', node: node.id, key: name, value: infer(value) });
    setKey('');
    setValue('');
  };
  const onIdKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') rename();
  };

  const family = familyOfComponent(node.component) ?? 'control';
  const params = Object.keys(node.params).sort();
  return (
    <Inspector family={family} title={node.id} impl={`${node.component}/${node.impl}`}>
      <div className="rg-editor-inspector__verdict" data-invalid={verdict === null ? undefined : true}>
        {verdict === null ? quiet : verdict.message}
      </div>
      <Input id={`${prefix}-id`} label="Node id" mono value={id} help="Unique in the pipeline." {...(idError === undefined ? {} : { error: idError })} onChange={(e) => setId(e.target.value)} onBlur={rename} onKeyDown={onIdKey} />
      <dl className="rg-editor-inspector__meta">
        <dt>Family</dt>
        <dd>{node.component}</dd>
        <dt>Implementation</dt>
        <dd>{node.impl}</dd>
      </dl>
      <h4 className="rg-editor-inspector__head">Inputs</h4>
      <ol className="rg-editor-inspector__inputs">
        {ports.inputs.map((kind, port) => {
          const from = node.inputs[port];
          const named = verdict !== null && verdict.port === port;
          const label = `port ${port}${kind === 'opaque' ? '' : ` (${PORT_LABEL[kind]})`}`;
          return (
            <li key={port} aria-label={`${label}, ${from === undefined ? 'empty' : `from ${from}`}`} data-invalid={named || undefined}>
              <span>
                {label}: {from === undefined ? <i>empty</i> : <code>{from}</code>}
              </span>
              {from === undefined || port !== node.inputs.length - 1 ? null : (
                <Button kind="quiet" size="s" icon="close" onClick={() => dispatch({ type: 'disconnect', node: node.id, port })}>
                  <span className="rg-visually-hidden">Remove the edge into port {port}</span>
                </Button>
              )}
              {/* Always there, one line: a verdict landing marks the row and moves nothing. The words are in the slot above. */}
              <small>{named ? 'The server names this edge.' : ''}</small>
            </li>
          );
        })}
      </ol>
      <h4 className="rg-editor-inspector__head">Parameters</h4>
      {params.length === 0 ? <p className="rg-editor-inspector__none">No parameter set.</p> : null}
      {params.map((name) => (
        <Parameter
          key={name}
          prefix={prefix}
          name={name}
          value={node.params[name]!}
          onSet={(v) => dispatch({ type: 'setParam', node: node.id, key: name, value: v })}
          onRemove={() => dispatch({ type: 'removeParam', node: node.id, key: name })}
        />
      ))}
      <div className="rg-editor-inspector__add">
        <Input id={`${prefix}-key`} label="New parameter" mono value={key} onChange={(e) => setKey(e.target.value)} />
        <Input id={`${prefix}-value`} label="Value" mono value={value} onChange={(e) => setValue(e.target.value)} />
        <Button kind="secondary" size="s" onClick={add}>
          Add parameter
        </Button>
      </div>
    </Inspector>
  );
}
