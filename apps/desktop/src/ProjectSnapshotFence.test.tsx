// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { ProjectDetail } from './Projects';
import type { Project, Snapshot, Task } from './types';

vi.mock('./App', () => ({ createAgent: vi.fn() }));
vi.mock('./client', () => ({ command: vi.fn() }));
vi.mock('./Connections', () => ({ Selection: () => null }));
vi.mock('./MemberBriefPanel', () => ({ MemberBriefPanel: () => null }));
vi.mock('./TaskJournal', () => ({ TaskJournal: () => null }));
vi.mock('./TaskEditor', () => ({ TaskEditor: () => null }));
vi.mock('./ObjectEditor', () => ({ ObjectEditor: () => null }));
vi.mock('./TaskDispatchControls', () => ({
  TaskDispatchControls: () => <div data-testid="dispatch-controls">派工控件</div>,
}));

const project = { id: 'project', name: '项目', goal: '', resources: [], context_id: null } as Project;
const task = {
  id: 'task', project_id: 'project', title: '任务', goal: '',
  scope: ['src'], acceptance: '验证通过', agent_ids: ['agent'],
  status: 'published', publication_epoch: 1,
} as Task;
function state(currentTask: Task): Snapshot {
  return {
    revision: 1, projects: [project], tasks: [currentTask],
    agents: [], agent_duties: [], connections: [], conversations: [],
    contexts: [], task_records: [],
    dispatch_attempts: [{
      id: 'attempt', task_id: 'task', project_id: 'project',
      agent_id: 'agent', publication_epoch: 1, status: 'pending', recorded_revision: 1,
    }],
  } as unknown as Snapshot;
}
const run = async (action: () => Promise<void>) => { await action(); };
const openChat = () => {};
afterEach(cleanup);

it('shows cached task evidence as unverified and hides dispatch controls until refreshed', () => {
  const data = state(task);
  const view = render(<ProjectDetail project={project} data={data}
    snapshotFresh={false} run={run} openChat={openChat} />);
  expect(screen.getByText(/派工记录尚未核实/)).toBeTruthy();
  expect(screen.queryByText(/已登记待派工（尚未执行）/)).toBeNull();
  screen.getByRole('button', { name: /任务/ }).click();
  expect(screen.getByText(/已暂停准入预检和派工登记/)).toBeTruthy();
  expect(screen.queryByTestId('dispatch-controls')).toBeNull();
  view.rerender(<ProjectDetail project={project} data={data}
    snapshotFresh={true} run={run} openChat={openChat} />);
  expect(screen.getByText(/已登记待派工（尚未执行）/)).toBeTruthy();
  expect(screen.getByTestId('dispatch-controls')).toBeTruthy();
});

it('does not allow publishing a draft using a stale snapshot revision', () => {
  const data = state({ ...task, status: 'draft' });
  const view = render(<ProjectDetail project={project} data={data}
    snapshotFresh={false} run={run} openChat={openChat} />);
  screen.getByRole('button', { name: /任务/ }).click();
  const publish = screen.getByRole('button', { name: '发布任务（不启动施工）' }) as HTMLButtonElement;
  expect(publish.disabled).toBe(true);
  view.rerender(<ProjectDetail project={project} data={data}
    snapshotFresh={true} run={run} openChat={openChat} />);
  expect(publish.disabled).toBe(false);
});
