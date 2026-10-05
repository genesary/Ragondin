import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { Button, Checkbox, Input, Inspector, Select, familyOfComponent } from '../../design/index.ts';
import type { Parameter as Served, ParameterKind, ParameterValue } from '../api/types.ts';
import { PORT_LABEL } from '../canvas/Port.tsx';
import { bool, float, formatFloat, int, list, str } from '../parameters.ts';
import { Words } from '../words.tsx';
import { freshId, type WireDocument, type WireNode } from './document.ts';
import type { NodePorts } from './ports.ts';
import type { EditorAction } from './store.ts';
import type { RunUpTo } from './prefix.ts';

/** What the server said about this node: its words, and the input port the edge it named enters, if it named one. */
export type NodeVerdict = { message: string; port: number | null } | null;

export type EditorInspectorProps = {
  doc: WireDocument;
  node: WireNode;
  ports: NodePorts;
  /** What the node takes, as `GET /workspace` serves it for its family and name; null when the capabilities say nothing about it. */
  parameters: readonly Served[] | null;
  /** Whether the server refused a save of this document: a required key still unset is then marked invalid, not only said to be required. */
  insisted: boolean;
  verdict: NodeVerdict;
  /** Where the server's verdict stands when it names nothing here: checking, valid, stopped elsewhere, or no verdict. */
  quiet: Quiet;
  dispatch: (action: EditorAction) => void;
  /** The node was renamed: the selection follows it. */
  onRenamed: (id: string) => void;
  /** Whether the node can be run up to, as the node menu says it. */
  run: RunUpTo;
  /** Opens the launch panel on the stored pipeline, cut at this node. */
  onRunUpTo: () => void;
};

type Kind = ParameterValue['kind'];

/** Where the document's verdict stands, for a node the server names nothing about. */
export type Quiet = 'checking' | 'valid' | 'invalid' | 'failed';

// What the verdict slot says of a node the server names nothing about. Valid is a small mark, not a sentence: nothing
// is wrong, so there is nothing to read.
const QUIET: Record<Exclude<Quiet, 'valid'>, string> = {
  checking: 'Checking with the server…',
  invalid: 'The server stopped at a problem elsewhere; it has not judged this node past it.',
  failed: 'No verdict: the request to the server failed.',
};

/** How long a value must rest while it is typed before it is committed: a pause, not every keystroke. */
export const PARAM_DEBOUNCE_MS = 400;

// Each kind ADR-C22's flat grammar has, as the person picks it. The order is
// the one a configuration most often needs them in.
const KINDS: readonly { value: Kind; label: string }[] = [
  { value: 'int', label: 'Integer' },
  { value: 'float', label: 'Float' },
  { value: 'string', label: 'Text' },
  { value: 'bool', label: 'Flag' },
  { value: 'list', label: 'List' },
];

const KIND_LABEL: Record<Kind, string> = Object.fromEntries(KINDS.map((k) => [k.value, k.label])) as Record<Kind, string>;

/** A value as its field shows it: a float with its fractional part, a list's items separated by commas. */
function textOf(value: ParameterValue): string {
  switch (value.kind) {
    case 'int':
    case 'string':
      return value.value;
    case 'float':
      return formatFloat(value.value);
    case 'bool':
      return String(value.value);
    case 'list':
      return value.value.map(textOf).join(', ');
  }
}

// The forms a person types: a whole number, signed or not; and a number
// with a point or an exponent, `1.5`, `.5`, `1.`, `1e3`.
const INTEGER = /^[-+]?\d+$/;
const NUMBER = /^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/;
const NOT_FINITE = /^[-+]?(inf|infinity|nan)$/i;
const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

/** Text read as one kind: the value, or why the kind cannot carry it, in words. */
type Read = { value: ParameterValue } | { error: string };

/**
 * Text read as one kind. A list's items are read one by one against `was`,
 * the items the list held. While the count is the same, an item is matched
 * to the one in its place: kept as it was when its text is unchanged, given
 * that item's kind when its text reads in it, so `[1, 0.5]` edited to
 * `1, 0.75` stays an integer and a float. When the count changes, places mean
 * nothing: an item is kept only when an item not yet matched has exactly its
 * text, and any other is read from its text, so `[1, 0.5, 2]` edited to
 * `1, 2` is two integers.
 */
function readAs(kind: Kind, text: string, was: readonly ParameterValue[] = []): Read {
  const t = text.trim();
  switch (kind) {
    case 'int': {
      if (!INTEGER.test(t)) return { error: `An integer is a whole number, such as 60: "${text}" is not one.` };
      const whole = BigInt(t);
      // Held as decimal text, never as a browser number, which would round it.
      if (whole < I64_MIN || whole > I64_MAX) return { error: `An integer has at most 64 bits: "${text}" is wider.` };
      return { value: int(whole.toString()) };
    }
    case 'float': {
      const number = Number(t);
      if (NOT_FINITE.test(t) || (NUMBER.test(t) && !Number.isFinite(number))) return { error: `A float is a finite number: "${text}" is not one.` };
      if (!NUMBER.test(t)) return { error: `A float is a number, such as 0.5: "${text}" is not one.` };
      return { value: float(number) };
    }
    case 'bool':
      if (t === 'true' || t === 'false') return { value: bool(t === 'true') };
      return { error: `A flag is true or false: "${text}" is neither.` };
    case 'string':
      return { value: str(text) };
    case 'list': {
      const read: ParameterValue[] = [];
      const items = text.split(',').map((i) => i.trim()).filter((i) => i !== '');
      const unmatched = [...was];
      for (const [at, item] of items.entries()) {
        if (items.length !== was.length) {
          const same = unmatched.findIndex((before) => before.kind !== 'list' && textOf(before) === item);
          if (same !== -1) {
            read.push(unmatched.splice(same, 1)[0]!);
            continue;
          }
          const one = infer(item);
          if ('error' in one) return one;
          read.push(one.value);
          continue;
        }
        const before = was[at];
        if (before !== undefined && before.kind !== 'list' && textOf(before) === item) {
          read.push(before);
          continue;
        }
        const kept = before === undefined || before.kind === 'list' ? null : readAs(before.kind, item);
        const one = kept !== null && 'value' in kept ? kept : infer(item);
        if ('error' in one) return one;
        read.push(one.value);
      }
      return { value: list(...read) };
    }
  }
}

/**
 * A value no kind was picked for: an integer when it is a whole number, a
 * float when it has a point or an exponent, else text as typed — `true`, or
 * text holding a comma, stays text unless the person picks another kind
 * (ADR-C40 § 8).
 */
function infer(text: string): Read {
  const t = text.trim();
  if (INTEGER.test(t)) return readAs('int', t);
  if (NUMBER.test(t)) return readAs('float', t);
  return readAs('string', text);
}

/**
 * Why a list's field cannot edit it, or null when it can. The field shows the
 * items separated by commas and reads them back split on commas and trimmed,
 * so a list holding a list would come back flattened, and text holding a
 * comma, or spaces at its ends, would come back split or trimmed.
 */
function readOnlyReason(value: ParameterValue): string | null {
  if (value.kind !== 'list') return null;
  if (value.value.some((item) => item.kind === 'list')) return 'A list holding a list is shown, not edited, here: its field would flatten it. Edit it in the file.';
  if (value.value.some((item) => item.kind === 'string' && (item.value.includes(',') || item.value.trim() !== item.value || item.value === ''))) {
    return 'A list holding text with a comma, or with spaces at its ends, is shown, not edited, here: its field would split or trim it. Edit it in the file.';
  }
  return null;
}

// The kind a served parameter is typed in, and how its row names it while it is not set.
const SERVED_KIND: Record<ParameterKind, { kind: Kind; label: string }> = {
  non_negative_integer: { kind: 'int', label: 'integer, zero or more' },
  string: { kind: 'string', label: 'text' },
  float: { kind: 'float', label: 'float' },
};
const REQUIRED = 'Required: the pipeline cannot be saved or run without it.';
const servedKind = (served: Served | undefined): Kind | null => (served === undefined ? null : SERVED_KIND[served.kind].kind);

/** What a served parameter is for, on a line of its own under its row, so an error on the field never hides it; its field names it in `aria-describedby`. */
function About({ id, served }: { id: string; served: Served | undefined }) {
  return served === undefined ? null : (
    <p id={id} className="rg-editor-inspector__about">
      {served.description}
    </p>
  );
}

/**
 * A parameter the implementation takes and the node does not set: an empty
 * field, typed in the kind it is served with, that sets it once a value is
 * entered — and, for text, to the empty text on Enter in the empty field,
 * never on leaving it. A required one says so as information; it is marked
 * invalid only once the server refused a save for want of it (`insisted`),
 * never before the person has done anything.
 */
function Unset({ prefix, served, insisted, onSet }: { prefix: string; served: Served; insisted: boolean; onSet: (v: ParameterValue) => void }) {
  const [text, setText] = useState('');
  const [error, setError] = useState<string | undefined>(undefined);
  const { kind, label } = SERVED_KIND[served.kind];
  const commit = (entered: boolean) => {
    if (text === '' || (kind !== 'string' && text.trim() === '')) {
      setError(undefined);
      if (entered && text === '' && kind === 'string') onSet(str(''));
      return;
    }
    const read = readAs(kind, text);
    if ('error' in read) return setError(read.error);
    setError(undefined);
    setText('');
    onSet(read.value);
  };
  const id = `${prefix}-param-${served.name}`;
  const invalid = error ?? (served.required && insisted ? REQUIRED : undefined);
  const info = served.required && invalid === undefined ? 'Required.' : undefined;
  const row = useErrorInView(error);
  return (
    <>
      <div ref={row} className="rg-editor-inspector__param" data-unset>
        <Input
          id={id}
          label={served.name}
          mono
          numeric={kind === 'int' || kind === 'float'}
          value={text}
          describedBy={`${id}-unset ${id}-about`}
          {...(invalid === undefined ? {} : { error: invalid })}
          {...(info === undefined ? {} : { help: info })}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => commit(false)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit(true);
          }}
        />
        <small id={`${id}-unset`} className="rg-editor-inspector__unset">
          Not set · {label}
          {kind === 'string' ? <span className="rg-visually-hidden">; Enter in the empty field sets it to the empty text</span> : null}
        </small>
      </div>
      <About id={`${id}-about`} served={served} />
    </>
  );
}

/** Brings a field's row, its error line included, into view inside the inspector's scrolling body once an error lands on it. */
function useErrorInView(error: unknown) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (error !== undefined) row.current?.scrollIntoView?.({ block: 'nearest' });
  }, [error]);
  return row;
}

function Parameter({ prefix, name, value, served, onSet, onRemove }: { prefix: string; name: string; value: ParameterValue; served: Kind | null; onSet: (v: ParameterValue) => void; onRemove: () => void }) {
  const about = served !== null;
  const [text, setText] = useState(textOf(value));
  const [error, setError] = useState<string | undefined>(undefined);
  const [kindError, setKindError] = useState<string | undefined>(undefined);
  const shown = textOf(value);
  // The text of the value this field itself last committed. Its coming back is not a change from outside, so the
  // field keeps what is typed — "1." stays "1.", never "1.0" under the next digit — and only another value, an undo
  // for one, replaces it.
  const mine = useRef<string | null>(null);
  useEffect(() => {
    const own = mine.current === shown;
    mine.current = null;
    if (!own) setText(shown);
  }, [shown]);
  const row = useErrorInView(error ?? kindError);
  const readOnly = readOnlyReason(value);
  // Leaving the field shows the value in its own form; a commit while it is typed leaves the text as typed.
  const commit = (leaving: boolean) => {
    // Tabbing through a field, or Enter on it, changes nothing it was not asked to.
    if (readOnly !== null || text === shown) return setError(undefined);
    const read = readAs(value.kind, text, value.kind === 'list' ? value.value : []);
    if ('error' in read) return setError(read.error);
    setError(undefined);
    setKindError(undefined);
    const next = textOf(read.value);
    if (next !== shown) {
      mine.current = next;
      onSet(read.value);
    }
    if (leaving) setText(next);
  };
  // While it is typed, a value its kind carries is committed once the typing rests; one it cannot carry waits for the
  // field to be left, so a value half typed ("1e" on the way to "1e3") is never called wrong. Text that reads as the
  // value already held — "1." once 1.0 is committed — has nothing to commit.
  const latest = useRef(commit);
  latest.current = commit;
  useEffect(() => {
    if (readOnly !== null || text === shown) return;
    const read = readAs(value.kind, text, value.kind === 'list' ? value.value : []);
    if ('error' in read || textOf(read.value) === shown) return;
    const timer = setTimeout(() => latest.current(false), PARAM_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [text, value, shown, readOnly]);
  // The value as it reads in the kind picked; refused in words when it cannot, and the kind kept.
  const rekind = (kind: Kind) => {
    const read = readAs(kind, textOf(value));
    if ('error' in read) return setKindError(read.error);
    setError(undefined);
    setKindError(undefined);
    onSet(read.value);
  };
  const id = `${prefix}-param-${name}`;
  // A served key is typed in its served kind: no other is offered, except the kind its value is written in when that
  // is another, so the value can be brought back to it.
  const offered = served === null ? KINDS : [...new Set([value.kind, served])].map((k) => ({ value: k, label: KIND_LABEL[k] }));
  const kind =
    offered.length === 1 ? (
      <span className="rg-editor-inspector__kind" title={`Kind of ${name}`}>
        {KIND_LABEL[value.kind]}
      </span>
    ) : (
      <Select
        id={`${id}-kind`}
        label={`Kind of ${name}`}
        options={offered}
        value={value.kind}
        disabled={readOnly !== null}
        {...(kindError === undefined ? {} : { error: kindError })}
        onChange={(e) => rekind(e.target.value as Kind)}
      />
    );
  const remove = (
    <Button kind="quiet" size="s" icon="close" onClick={onRemove}>
      <span className="rg-visually-hidden">Remove {name}</span>
    </Button>
  );
  if (value.kind === 'bool') {
    return (
      <div ref={row} className="rg-editor-inspector__param">
        <Checkbox label={name} checked={value.value} onChange={(checked) => onSet(bool(checked))} />
        {kind}
        {remove}
      </div>
    );
  }
  return (
    <div ref={row} className="rg-editor-inspector__param">
      <Input
        id={id}
        label={name}
        mono
        {...(about ? { describedBy: `${id}-about` } : {})}
        numeric={value.kind === 'int' || value.kind === 'float'}
        value={text}
        readOnly={readOnly !== null}
        // The line is always there, so an error replaces it rather than pushing the fields below down.
        help={readOnly ?? (value.kind === 'list' ? 'A list: items separated by commas.' : ' ')}
        {...(error === undefined ? {} : { error: <Words text={error} /> })}
        onChange={(e) => {
          if (readOnly === null) setText(e.target.value);
        }}
        onBlur={() => commit(true)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit(false);
        }}
      />
      {kind}
      {remove}
    </div>
  );
}

// What the new parameter's kind starts as: none picked, read from the value.
const FROM_VALUE = '';

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
export function EditorInspector({ doc, node, ports, parameters, insisted, verdict, quiet, dispatch, onRenamed, run, onRunUpTo }: EditorInspectorProps) {
  const prefix = useId();
  const [id, setId] = useState(node.id);
  const [idError, setIdError] = useState<string | undefined>(undefined);
  const [key, setKey] = useState('');
  const [value, setValue] = useState('');
  const [newKind, setNewKind] = useState<Kind | typeof FROM_VALUE>(FROM_VALUE);
  const [valueError, setValueError] = useState<string | undefined>(undefined);
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
    const read = newKind === FROM_VALUE ? infer(value) : readAs(newKind, value);
    if ('error' in read) return setValueError(read.error);
    setValueError(undefined);
    dispatch({ type: 'setParam', node: node.id, key: name, value: read.value });
    setKey('');
    setValue('');
    setNewKind(FROM_VALUE);
  };
  const onIdKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') rename();
  };

  const family = familyOfComponent(node.component) ?? 'control';
  // Every key the node sets and every key it takes, set or not, in one key order.
  const served = new Map((parameters ?? []).map((p) => [p.name, p]));
  const params = [...new Set([...Object.keys(node.params), ...served.keys()])].sort();
  return (
    <Inspector
      family={family}
      title={node.id}
      impl={`${node.component}/${node.impl}`}
      footer={
        // The secondary action: the node menu's "Run up to this node", refused for the same reason.
        run.kind === 'open' ? (
          <Button kind="secondary" size="s" icon="prefix" onClick={onRunUpTo}>
            Run up to here
          </Button>
        ) : (
          <Button kind="secondary" size="s" icon="prefix" disabled disabledReason={run.reason}>
            Run up to here
          </Button>
        )
      }
    >
      <div className="rg-editor-inspector__verdict" data-invalid={verdict === null ? undefined : true} data-valid={verdict === null && quiet === 'valid' ? true : undefined}>
        {verdict !== null ? <Words text={verdict.message} /> : quiet === 'valid' ? 'Valid' : QUIET[quiet]}
      </div>
      <Input id={`${prefix}-id`} label="Node id" mono value={id} help="Unique in the pipeline." {...(idError === undefined ? {} : { error: <Words text={idError} /> })} onChange={(e) => setId(e.target.value)} onBlur={rename} onKeyDown={onIdKey} />
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
          // Counted from one, as a person counts; the server's own words keep its port numbers.
          const label = `Input ${port + 1}${kind === 'opaque' ? '' : ` (${PORT_LABEL[kind]})`}`;
          return (
            <li key={port} aria-label={`${label}, ${from === undefined ? 'not connected' : `from ${from}`}`} data-invalid={named || undefined}>
              <span>
                {label}: {from === undefined ? <i>not connected</i> : <code>{from}</code>}
              </span>
              {from === undefined || port !== node.inputs.length - 1 ? null : (
                <Button kind="quiet" size="s" icon="close" onClick={() => dispatch({ type: 'disconnect', node: node.id, port })}>
                  <span className="rg-visually-hidden">Remove the edge into input {port + 1}</span>
                </Button>
              )}
              {/* Always there, one line: a verdict landing marks the row and moves nothing. The words are in the slot above. */}
              <small>{named ? 'The server names this edge.' : ''}</small>
            </li>
          );
        })}
      </ol>
      <h4 className="rg-editor-inspector__head">Parameters</h4>
      {params.length === 0 ? <p className="rg-editor-inspector__none">{parameters === null ? 'No parameter set.' : 'This implementation takes no parameter.'}</p> : null}
      {params.map((name) => {
        const value = node.params[name];
        const set = (v: ParameterValue) => dispatch({ type: 'setParam', node: node.id, key: name, value: v });
        if (value === undefined) return <Unset key={name} prefix={prefix} served={served.get(name)!} insisted={insisted} onSet={set} />;
        return (
          <div key={name}>
            <Parameter prefix={prefix} name={name} value={value} served={servedKind(served.get(name))} onSet={set} onRemove={() => dispatch({ type: 'removeParam', node: node.id, key: name })} />
            <About id={`${prefix}-param-${name}-about`} served={served.get(name)} />
          </div>
        );
      })}
      <div className="rg-editor-inspector__add">
        <Input id={`${prefix}-key`} label="New parameter" mono value={key} onChange={(e) => setKey(e.target.value)} />
        <Select
          id={`${prefix}-kind`}
          label="Kind"
          options={[{ value: FROM_VALUE, label: 'From the value' }, ...KINDS]}
          value={newKind}
          help="From the value: a whole number is an integer, one with a point or an exponent a float, anything else text."
          onChange={(e) => setNewKind(e.target.value as Kind | typeof FROM_VALUE)}
        />
        <Input id={`${prefix}-value`} label="Value" mono value={value} {...(valueError === undefined ? {} : { error: valueError })} onChange={(e) => setValue(e.target.value)} />
        <Button kind="secondary" size="s" onClick={add}>
          Add parameter
        </Button>
      </div>
    </Inspector>
  );
}
