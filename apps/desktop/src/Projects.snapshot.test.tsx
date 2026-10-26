// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { ProjectDetail } from './Projects';
import type { Project, Snapshot } from './types';
import type { Run } from './App';

vi.mock('./ObjectEditor', () => ({
  ObjectEditor: ({run}:{run:Run}) => <form onSubmit={e=>{e.preventDefault();void run(async()=>{});}}><button type="submit">修改项目</button></form>,
}));
vi.mock('./TaskEditor', () => ({
  TaskEditor: ({run}:{run:Run}) => <form onSubmit={e=>{e.preventDefault();void run(async()=>{});}}><button type="submit">保存任务</button></form>,
}));
vi.mock('./TaskJournal', () => ({
  TaskJournal: ({run}:{run:Run}) => <form onSubmit={e=>{e.preventDefault();void run(async()=>{});}}><button type="submit">添加进展或交接</button></form>,
}));
vi.mock('./TaskDispatchStatus', () => ({
  TaskDispatchStatus: () => <span>尚未派工</span>,
}));

const project = { id: 'project-1', name: '项目 A', goal: '', resources: [], context_id: null } as Project;
const state = {
  revision: 4,
  projects: [project],
  agents: [{ id: 'agent-1', name: '施工成员', project_id: 'project-1', role: 'worker', connection_id: 'api', model_id: 'model-a' }],
  agent_duties: [],
  connections: [{ id: 'api', name: 'API A', base_url: 'https://example.invalid', models: ['model-a'], enabled: true }],
  contexts: [],
  conversations: [],
  tasks: [{ id: 'task-1', project_id: 'project-1', title: '任务 A', status: 'draft', agent_ids: [], scope: [], acceptance: '', goal: '' }],
  task_records: [],
  dispatch_attempts: [],
} as unknown as Snapshot;
afterEach(cleanup);

const controls = ['修改项目', '登记职责', '保存任务', '添加进展或交接', '保存草案'];
function assertPaused(paused: boolean) {
  for (const label of controls) {
    const fieldset = screen.getByText(label).closest('fieldset') as HTMLFieldSetElement | null;
    expect(fieldset?.disabled, label).toBe(paused);
  }
  expect((screen.getByRole('combobox', { name: '关联上下文' }) as HTMLSelectElement).disabled).toBe(paused);
}

it('keeps project and task evidence visible but pauses every project mutation on a stale Host snapshot', () => {
  const run = vi.fn(async () => {});
  const view = render(<ProjectDetail project={project} data={state} snapshotFresh={true} run={run} openChat={vi.fn()}/>);
  fireEvent.click(screen.getByRole('button', { name: /任务 A/ }));
  assertPaused(false);

  view.rerender(<ProjectDetail project={project} data={state} snapshotFresh={false} run={run} openChat={vi.fn()}/>);
  expect(screen.getByText(/项目与任务资料来自上次成功快照/)).toBeTruthy();
  expect(screen.getByRole('heading', { name: '任务 A' })).toBeTruthy();
  assertPaused(true);

  // Even a synthetic form submit must not bypass the stale-state fence.
  fireEvent.submit(screen.getByText('保存草案').closest('form')!);
  fireEvent.submit(screen.getByText('登记职责').closest('form')!);
  expect(run).not.toHaveBeenCalled();

  view.rerender(<ProjectDetail project={project} data={state} snapshotFresh={true} run={run} openChat={vi.fn()}/>);
  assertPaused(false);
});

// Disabled fieldsets are a visual interaction guard, not an authorization
// boundary: a synthetic submit can still reach mounted child handlers.
it('rejects retained editor submit callbacks while the Host snapshot is stale', () => {
  const run = vi.fn(async () => {});
  const view = render(<ProjectDetail project={project} data={state} snapshotFresh={true} run={run} openChat={vi.fn()}/>);
  fireEvent.click(screen.getByRole('button', { name: /任务 A/ }));
  view.rerender(<ProjectDetail project={project} data={state} snapshotFresh={false} run={run} openChat={vi.fn()}/>);
  for (const label of ['修改项目', '保存任务', '添加进展或交接']) {
    fireEvent.submit(screen.getByText(label).closest('form')!);
  }
  expect(run).not.toHaveBeenCalled();

  view.rerender(<ProjectDetail project={project} data={state} snapshotFresh={true} run={run} openChat={vi.fn()}/>);
  for (const label of ['修改项目', '保存任务', '添加进展或交接']) {
    fireEvent.submit(screen.getByText(label).closest('form')!);
  }
  expect(run).toHaveBeenCalledTimes(3);
});
