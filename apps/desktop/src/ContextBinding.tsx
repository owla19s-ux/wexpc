import { useRef, useState } from 'react';
import { command } from './client';
import type { Run } from './App';
import type { Context } from './types';

type Binding = { value: string; revision: number };

export function ContextBinding({ kind, objectId, contextId, contexts, revision, snapshotFresh, run }: {
  kind: 'project' | 'conversation';
  objectId: string;
  contextId: string | null;
  contexts: Context[];
  revision: number;
  snapshotFresh: boolean;
  run: Run;
}) {
  const [draft, setDraft] = useState<Binding | null>(null);
  const [saving, setSaving] = useState(false);
  const inFlight = useRef(false);
  const value = draft?.value ?? contextId ?? '';
  const missing = value !== '' && !contexts.some(context => context.id === value);

  function change(value: string) {
    if (!snapshotFresh || inFlight.current) return;
    const requested = { value, revision: draft?.revision ?? revision };
    setDraft(requested);
    inFlight.current = true;
    setSaving(true);
    void run(async () => {
      await command('bind_' + kind + '_context', {
        revision: requested.revision,
        [kind + 'Id']: objectId,
        contextId: requested.value || null,
      });
      setDraft(null);
    }).finally(() => {
      inFlight.current = false;
      setSaving(false);
    });
  }

  return <div>
    <label>关联上下文<select disabled={!snapshotFresh || saving} value={value}
      onChange={event => change(event.target.value)}>
      <option value="">{kind === 'conversation' ? '独立对话，不关联上下文' : '暂不关联'}</option>
      {contexts.map(context => <option key={context.id} value={context.id}>{context.name}</option>)}
      {missing && <option value={value}>原选择已不可用（需核查）</option>}
    </select></label>
    {draft && !saving && <p role="status">
      关联选择尚未确认，已保留供核对。请重新读取资料后放弃原选择，再按最新资料重新选择；不会自动重试。
      <button type="button" disabled={!snapshotFresh}
        onClick={() => setDraft(null)}>放弃未确认选择</button>
    </p>}
  </div>;
}
