// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Organization } from './Organization';
import { ProjectDetail } from './Projects';
import { App, type Run } from './App';
import type { Snapshot } from './types';

const { ipc } = vi.hoisted(() => ({ ipc: vi.fn() }));
vi.mock('./client', () => ({
  command: ipc, desktopAvailable: true,
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./Chat', () => ({ Chat: () => null }));

function snapshot(revision = 5): Snapshot {
  return {
    revision,
    spaces: [{ id: 'home', name: '原空间' }, { id: 'work', name: '目标空间' }],
    groups: [],
    contexts: [
      { id: 'a', name: '上下文 A', goal: '', resources: [] },
      { id: 'b', name: '上下文 B', goal: '', resources: [] },
    ],
    placements: [
      { context_id: 'a', space_id: 'home', group_id: null },
      { context_id: 'b', space_id: 'work', group_id: null },
    ],
    projects: [{ id: 'p', name: '项目', goal: '', resources: [], context_id: null }],
    conversations: [{
      id: 'chat', title: '独立研究', project_id: null, context_id: null,
      connection_id: 'api', model_id: 'model',
    }],
    connections: [{
      id: 'api', name: 'API', base_url: 'https://example.invalid',
      models: ['model'], enabled: true,
    }],
    agents: [], agent_duties: [], tasks: [], dispatch_attempts: [],
    turns: [], task_records: [], unsaved_turn_ids: [],
  };
}

const run: Run = async action => { try { await action(); } catch { /* App reports errors. */ } };
afterEach(() => { cleanup(); ipc.mockReset(); vi.restoreAllMocks(); });

it('sends the revision where a move draft began even if another snapshot arrives', async () => {
  ipc.mockResolvedValue(undefined);
  const data = snapshot();
  const view = render(<Organization data={data} run={run} />);
  fireEvent.change(screen.getByRole('combobox', { name: '目标空间' }), { target: { value: 'work' } });
  view.rerender(<Organization data={snapshot(6)} run={run} />);
  fireEvent.submit(screen.getByRole('button', { name: '保存位置' }).closest('form')!);
  await waitFor(() => expect(ipc).toHaveBeenCalledWith('move_context', {
    revision: 5, contextId: 'a', spaceId: 'work', groupId: null,
  }));
});

it('keeps a rejected move draft visible if the refreshed placement moved elsewhere', async () => {
  ipc.mockRejectedValue(new Error('REVISION_CONFLICT'));
  const data = snapshot();
  const view = render(<Organization data={data} run={run} />);
  fireEvent.change(screen.getByRole('combobox', { name: '目标空间' }), { target: { value: 'work' } });
  fireEvent.submit(screen.getByRole('button', { name: '保存位置' }).closest('form')!);
  await waitFor(() => expect(ipc).toHaveBeenCalledTimes(1));
  const newer = snapshot(6);
  newer.placements[0] = { context_id: 'a', space_id: 'work', group_id: null };
  view.rerender(<Organization data={newer} run={run} />);
  expect((screen.getByRole('combobox', { name: '目标空间' }) as HTMLSelectElement).value).toBe('work');
  expect(ipc).toHaveBeenCalledTimes(1);
});

it('passes the displayed project revision when changing its context', async () => {
  ipc.mockResolvedValue(undefined);
  const data = snapshot();
  render(<ProjectDetail project={data.projects[0]} data={data} snapshotFresh={true}
    run={run} openChat={() => {}} />);
  fireEvent.change(screen.getByRole('combobox', { name: '关联上下文' }), { target: { value: 'b' } });
  await waitFor(() => expect(ipc).toHaveBeenCalledWith('bind_project_context', {
    revision: 5, projectId: 'p', contextId: 'b',
  }));
});

it('retains a failed project selection after refresh without replaying it', async () => {
  ipc.mockRejectedValue(new Error('REVISION_CONFLICT'));
  const data = snapshot();
  const view = render(<ProjectDetail project={data.projects[0]} data={data} snapshotFresh={true}
    run={run} openChat={() => {}} />);
  fireEvent.change(screen.getByRole('combobox', { name: '关联上下文' }), { target: { value: 'b' } });
  await waitFor(() => expect(ipc).toHaveBeenCalledTimes(1));
  const newer = snapshot(6);
  newer.projects[0].context_id = 'a';
  view.rerender(<ProjectDetail project={newer.projects[0]} data={newer} snapshotFresh={true}
    run={run} openChat={() => {}} />);
  expect((screen.getByRole('combobox', { name: '关联上下文' }) as HTMLSelectElement).value).toBe('b');
  expect(ipc).toHaveBeenCalledTimes(1);
  fireEvent.change(screen.getByRole('combobox', { name: '关联上下文' }), { target: { value: 'a' } });
  await waitFor(() => expect(ipc).toHaveBeenLastCalledWith('bind_project_context', {
    revision: 5, projectId: 'p', contextId: 'a',
  }));
});

it('passes the displayed conversation revision through the real App binding control', async () => {
  const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') return snapshot();
    if (name === 'bind_conversation_context') return;
    throw new Error(name);
  });
  render(<App />);
  await screen.findByRole('button', { name: '独立研究' });
  fireEvent.click(screen.getByRole('button', { name: '独立研究' }));
  fireEvent.change(screen.getByRole('combobox', { name: '关联上下文' }), { target: { value: 'b' } });
  await waitFor(() => expect(ipc).toHaveBeenCalledWith('bind_conversation_context', {
    revision: 5, conversationId: 'chat', contextId: 'b',
  }));
  expect(errors.mock.calls.filter(([message]) => String(message).includes('same key'))).toEqual([]);
});

it('blocks synthetic move submission when the authoritative snapshot is stale', () => {
  render(<Organization data={snapshot()} snapshotFresh={false} run={run} />);
  fireEvent.submit(screen.getByRole('button', { name: '保存位置' }).closest('form')!);
  expect(ipc).not.toHaveBeenCalled();
});

it('allows only one move request while the previous response is pending', async () => {
  let finish!: () => void;
  ipc.mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  render(<Organization data={snapshot()} run={run} />);
  const form = screen.getByRole('button', { name: '保存位置' }).closest('form')!;
  fireEvent.submit(form);
  fireEvent.submit(form);
  expect(ipc).toHaveBeenCalledTimes(1);
  finish();
  await waitFor(() => expect((screen.getByRole('button', { name: '保存位置' }) as HTMLButtonElement).disabled).toBe(false));
});
