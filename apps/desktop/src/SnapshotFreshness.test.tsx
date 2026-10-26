// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import type { Snapshot } from './types';

const { ipc } = vi.hoisted(() => ({ ipc: vi.fn() }));
vi.mock('./client', () => ({
  desktopAvailable: true,
  command: ipc,
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./Connections', () => ({
  ConnectionForm: () => null, GithubConnection: () => null, Selection: () => null,
}));
vi.mock('./Projects', () => ({
  ProjectDetail: ({ snapshotFresh }: { snapshotFresh: boolean }) =>
    <div data-testid="project-freshness">{snapshotFresh ? 'fresh' : 'stale'}</div>,
}));
vi.mock('./Chat', () => ({ Chat: () => null }));
vi.mock('./Organization', () => ({ Organization: () => null }));

const project = { id: 'project-1', name: '测试项目', goal: '', resources: [], context_id: null };
function snapshot(status: 'pending' | 'outcome_unknown'): Snapshot {
  return {
    revision: status === 'pending' ? 1 : 2,
    projects: [project],
    connections: [{ id: 'conn-1', name: 'API', base_url: 'https://example.invalid',
      enabled: true, models: ['model'] }],
    conversations: [], contexts: [], turns: [],
    tasks: [{ id: 'task-1', project_id: 'project-1', title: '任务', goal: '',
      scope: ['src'], acceptance: '通过', agent_ids: ['agent-1'],
      status: 'published', publication_epoch: 1 }],
    dispatch_attempts: [{ id: 'attempt-1', project_id: 'project-1', task_id: 'task-1',
      agent_id: 'agent-1', publication_epoch: 1, status, recorded_revision: 1 }],
  } as unknown as Snapshot;
}
afterEach(() => { cleanup(); ipc.mockReset(); });

it('marks cached task evidence stale after snapshot failure and restores it only on a successful read', async () => {
  let reads = 0;
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return snapshot('pending');
      if (reads === 2) throw new Error('snapshot unavailable');
      return snapshot('outcome_unknown');
    }
    if (name === 'set_connection_enabled') return;
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getByRole('button', { name: '连接与服务' }));
  fireEvent.click(screen.getByRole('button', { name: '停用' }));
  await screen.findByText(/快照读取未核实/);
  expect(screen.getByText(/任务\/派工状态当前未核实/)).toBeTruthy();
  expect(screen.queryByText(/1 个待领取/)).toBeNull();

  fireEvent.click(screen.getByRole('button', { name: '项目' }));
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  expect(screen.getByTestId('project-freshness').textContent).toBe('stale');

  fireEvent.click(screen.getByRole('button', { name: '重新读取快照' }));
  await waitFor(() => expect(screen.getByTestId('project-freshness').textContent).toBe('fresh'));
  expect(screen.getByText(/1 个结果未知/)).toBeTruthy();
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
  expect(reads).toBe(3);
});

it('does not invalidate a verified snapshot when the action fails before refresh', async () => {
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') return snapshot('pending');
    if (name === 'set_connection_enabled') throw new Error('operation denied');
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getByRole('button', { name: '连接与服务' }));
  fireEvent.click(screen.getByRole('button', { name: '停用' }));
  await screen.findByText(/operation denied/);
  expect(screen.getByText(/1 个待领取/)).toBeTruthy();
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
});
