// Opening another pipeline from the editor: every pipeline of the workspace,
// the ones this browser opened recently first. Choosing one moves the address
// to it, which flushes the editor open now and opens the one chosen.
import { Select } from '../../design/index.ts';
import { navigate } from '../routes.ts';
import { recentPipelines } from './recent.ts';

/** The names in the order the picker offers them: the recent ones still in the workspace, newest first, then the rest as listed. */
export function pickerOrder(names: readonly string[], recent: readonly string[] = recentPipelines()): string[] {
  const held = recent.filter((n) => names.includes(n));
  return [...held, ...names.filter((n) => !held.includes(n))];
}

export function PipelinePicker({ id, names, current }: { id: string; names: readonly string[]; current: string | null }) {
  const order = pickerOrder(names);
  const recent = new Set(recentPipelines());
  const options = [
    ...(current === null || !order.includes(current) ? [{ value: '', label: 'Choose a pipeline', disabled: true }] : []),
    ...order.map((n) => ({ value: n, label: recent.has(n) && n !== current ? `${n} · recent` : n })),
  ];
  return (
    <Select
      id={id}
      label="Open a pipeline"
      value={current !== null && order.includes(current) ? current : ''}
      options={options}
      onChange={(event) => {
        if (event.target.value !== '' && event.target.value !== current) navigate({ screen: 'editor', name: event.target.value });
      }}
    />
  );
}
