// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import type { Snapshot } from './types';

const { ipc, finishedRuns } = vi.hoisted(() => ({ ipc: vi.fn(), finishedRuns: vi.fn() }));
vi.mock('./client', () => ({
  desktopAvailable: true,
  command: ipc,
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./Connections', async (importOriginal) => ({
  ...await importOriginal<typeof import('./Connections')>(),
  ConnectionForm: () => null, GithubConnection: () => null, Selection: () => null,
  VerifiedConnectionCreation: () => null,
}));
vi.mock('./Projects', () => ({
  ProjectDetail: ({ snapshotFresh, run }: {
    snapshotFresh: boolean; run: (action: () => Promise<void>) => Promise<void>;
  }) =>
    <div data-testid="project-freshness">
      {snapshotFresh ? 'fresh' : 'stale'}
      <button disabled={!snapshotFresh} onClick={() => void run(() => ipc('queue_dispatch_attempt')).then(finishedRuns)}>
        登记派工
      </button>
      <button onClick={() => void run(async () => {}).then(finishedRuns)}>并发刷新</button>
    </div>,
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
afterEach(() => { cleanup(); ipc.mockReset(); finishedRuns.mockReset(); });

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
  expect(screen.getByTestId('project-freshness').textContent).toContain('stale');
  expect((screen.getByRole('button', { name: '登记派工' }) as HTMLButtonElement).disabled).toBe(true);

  fireEvent.click(screen.getByRole('button', { name: '重新读取快照' }));
  await waitFor(() => expect(screen.getByTestId('project-freshness').textContent).toContain('fresh'));
  expect(screen.getByText(/1 个结果未知/)).toBeTruthy();
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
  expect(reads).toBe(3);
});

it('reconciles even a rejected action and keeps a verified snapshot when read succeeds', async () => {
  let reads = 0;
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') { reads++; return snapshot('pending'); }
    if (name === 'set_connection_enabled') throw new Error('operation denied');
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getByRole('button', { name: '连接与服务' }));
  fireEvent.click(screen.getByRole('button', { name: '停用' }));
  await screen.findByText(/operation denied/);
  expect(reads).toBe(2);
  expect(screen.getByText(/1 个待领取/)).toBeTruthy();
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
});

it('fences retry after dispatch persists but IPC response and reconciliation both fail', async () => {
  let reads = 0;
  let dispatches = 0;
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return snapshot('pending');
      if (reads === 2) throw new Error('snapshot offline');
      return snapshot('outcome_unknown');
    }
    if (name === 'queue_dispatch_attempt') {
      dispatches++;
      throw new Error('IPC response lost after durable dispatch');
    }
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await screen.findByText(/IPC response lost after durable dispatch/);
  await screen.findByText(/快照读取未核实/);
  expect((screen.getByRole('button', { name: '登记派工' }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByText(/1 个待领取/)).toBeNull();
  expect(dispatches).toBe(1);

  fireEvent.click(screen.getByRole('button', { name: '重新读取快照' }));
  await screen.findByText(/1 个结果未知/);
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
  expect(dispatches).toBe(1);
  expect(reads).toBe(3);
});

it('preserves the original IPC error while showing the Host state after a successful recheck', async () => {
  let reads = 0;
  ipc.mockImplementation(async (name: string) => {
    if (name === 'snapshot') { reads++; return snapshot(reads === 1 ? 'pending' : 'outcome_unknown'); }
    if (name === 'queue_dispatch_attempt') throw new Error('dispatch response lost');
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await screen.findByText(/dispatch response lost/);
  await screen.findByText(/1 个结果未知/);
  expect(screen.queryByText(/快照读取未核实/)).toBeNull();
  expect(reads).toBe(2);
});


it('waits for the newest snapshot when dispatch reconciliation is superseded', async () => {
  let reads = 0;
  let dispatches = 0;
  let resolveA: ((value: Snapshot) => void) | undefined;
  let resolveB: ((value: Snapshot) => void) | undefined;
  ipc.mockImplementation((name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return Promise.resolve(snapshot('pending'));
      if (reads === 2) return new Promise<Snapshot>(resolve => { resolveA = resolve; });
      if (reads === 3) return new Promise<Snapshot>(resolve => { resolveB = resolve; });
      throw new Error('unexpected snapshot read');
    }
    if (name === 'queue_dispatch_attempt') {
      dispatches++;
      return Promise.reject(new Error('IPC response lost'));
    }
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await waitFor(() => expect(reads).toBe(2));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(3));
  // A has a valid payload, but B has superseded it and remains pending.
  await act(async () => { resolveA?.(snapshot('outcome_unknown')); });
  expect(finishedRuns).not.toHaveBeenCalled();
  expect(screen.getByText(/1 个待领取/)).toBeTruthy();
  await act(async () => { resolveB?.(snapshot('outcome_unknown')); });
  await waitFor(() => expect(finishedRuns).toHaveBeenCalledTimes(2));
  expect(screen.getByText(/1 个结果未知/)).toBeTruthy();
  expect(dispatches).toBe(1);
});

it('keeps evidence stale when the superseding snapshot fails', async () => {
  let reads = 0;
  let resolveA: ((value: Snapshot) => void) | undefined;
  let rejectB: ((reason: Error) => void) | undefined;
  ipc.mockImplementation((name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return Promise.resolve(snapshot('pending'));
      if (reads === 2) return new Promise<Snapshot>(resolve => { resolveA = resolve; });
      if (reads === 3) return new Promise<Snapshot>((_, reject) => { rejectB = reject; });
      throw new Error('unexpected snapshot read');
    }
    if (name === 'queue_dispatch_attempt') return Promise.reject(new Error('IPC response lost'));
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await waitFor(() => expect(reads).toBe(2));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(3));
  await act(async () => { resolveA?.(snapshot('pending')); });
  expect(finishedRuns).not.toHaveBeenCalled();
  await act(async () => { rejectB?.(new Error('latest snapshot unavailable')); });
  await waitFor(() => expect(finishedRuns).toHaveBeenCalledTimes(2));
  expect(screen.getByText(/任务\/派工状态当前未核实/)).toBeTruthy();
  expect((screen.getByRole('button', { name: '登记派工' }) as HTMLButtonElement).disabled).toBe(true);
});

it('settles the original dispatch after a newer verified snapshot even when the old IPC never returns', async () => {
  let reads = 0;
  let dispatches = 0;
  let resolveB: ((value: Snapshot) => void) | undefined;
  ipc.mockImplementation((name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return Promise.resolve(snapshot('pending'));
      if (reads === 2) return new Promise<Snapshot>(() => {}); // A never settles.
      if (reads === 3) return new Promise<Snapshot>(resolve => { resolveB = resolve; });
      throw new Error('unexpected snapshot read');
    }
    if (name === 'queue_dispatch_attempt') {
      dispatches++;
      return Promise.reject(new Error('dispatch response lost'));
    }
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await waitFor(() => expect(reads).toBe(2));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(3));
  expect(finishedRuns).not.toHaveBeenCalled();
  await act(async () => { resolveB?.(snapshot('outcome_unknown')); });
  await waitFor(() => expect(finishedRuns).toHaveBeenCalledTimes(2));
  expect(screen.getByText(/1 个结果未知/)).toBeTruthy();
  expect(dispatches).toBe(1);
});

it('settles the original dispatch as unverified when a newer snapshot fails and old IPC hangs', async () => {
  let reads = 0;
  let dispatches = 0;
  let rejectB: ((reason: Error) => void) | undefined;
  ipc.mockImplementation((name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return Promise.resolve(snapshot('pending'));
      if (reads === 2) return new Promise<Snapshot>(() => {}); // A never settles.
      if (reads === 3) return new Promise<Snapshot>((_, reject) => { rejectB = reject; });
      throw new Error('unexpected snapshot read');
    }
    if (name === 'queue_dispatch_attempt') {
      dispatches++;
      return Promise.reject(new Error('dispatch response lost'));
    }
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await waitFor(() => expect(reads).toBe(2));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(3));
  expect(finishedRuns).not.toHaveBeenCalled();
  await act(async () => { rejectB?.(new Error('latest snapshot unavailable')); });
  await waitFor(() => expect(finishedRuns).toHaveBeenCalledTimes(2));
  expect(screen.getByText(/任务\/派工状态当前未核实/)).toBeTruthy();
  expect((screen.getByRole('button', { name: '登记派工' }) as HTMLButtonElement).disabled).toBe(true);
  expect(dispatches).toBe(1);
});

it('settles A and B callers via C without waiting for either superseded IPC', async () => {
  let reads = 0;
  let resolveC: ((value: Snapshot) => void) | undefined;
  ipc.mockImplementation((name: string) => {
    if (name === 'snapshot') {
      reads++;
      if (reads === 1) return Promise.resolve(snapshot('pending'));
      if (reads === 2 || reads === 3) return new Promise<Snapshot>(() => {});
      if (reads === 4) return new Promise<Snapshot>(resolve => { resolveC = resolve; });
      throw new Error('unexpected snapshot read');
    }
    if (name === 'queue_dispatch_attempt') return Promise.reject(new Error('dispatch response lost'));
    throw new Error(name);
  });
  render(<App />);
  await screen.findByText(/1 个待领取/);
  fireEvent.click(screen.getAllByRole('button', { name: '测试项目' })[0]);
  fireEvent.click(screen.getByRole('button', { name: '登记派工' }));
  await waitFor(() => expect(reads).toBe(2));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(3));
  fireEvent.click(screen.getByRole('button', { name: '并发刷新' }));
  await waitFor(() => expect(reads).toBe(4));
  expect(finishedRuns).not.toHaveBeenCalled();
  await act(async () => { resolveC?.(snapshot('outcome_unknown')); });
  await waitFor(() => expect(finishedRuns).toHaveBeenCalledTimes(3));
  expect(screen.getByText(/1 个结果未知/)).toBeTruthy();
});
