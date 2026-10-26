// @vitest-environment happy-dom
import { afterEach, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { TaskDispatchStatus } from './TaskDispatchStatus';
import type { DispatchAttempt, Task } from './types';

const task: Task = {
  id: 'task-1', project_id: 'project-a', title: '任务', goal: '',
  scope: ['src'], acceptance: '测试通过', agent_ids: ['agent-a'],
  status: 'published', publication_epoch: 3,
};
const attempt = (status: DispatchAttempt['status'], epoch = 3): DispatchAttempt => ({
  id: 'attempt', task_id: 'task-1', project_id: 'project-a',
  agent_id: 'agent-a', publication_epoch: epoch, status, recorded_revision: 9,
});
afterEach(cleanup);

it('does not describe a pending, claimed, or unknown attempt as undispatched', () => {
  const view = render(<TaskDispatchStatus task={task} attempts={[attempt('pending')]} />);
  expect(screen.getByText('已登记待派工（尚未执行）')).toBeTruthy();
  view.rerender(<TaskDispatchStatus task={task} attempts={[attempt('in_flight')]} />);
  expect(screen.getByText('已领取（执行结果未确认）')).toBeTruthy();
  view.rerender(<TaskDispatchStatus task={task} attempts={[attempt('outcome_unknown')]} />);
  expect(screen.getByText('派工结果未知（需核查，不可盲目重试）')).toBeTruthy();
});

it('does not project historical or other-project attempts into the current publication', () => {
  render(<TaskDispatchStatus task={task} attempts={[
    attempt('in_flight', 2),
    { ...attempt('outcome_unknown'), project_id: 'project-b' },
  ]} />);
  expect(screen.getByText('已发布（尚未登记派工）')).toBeTruthy();
});

it('does not infer that no dispatch exists when evidence is absent', () => {
  const view = render(<TaskDispatchStatus task={task} />);
  expect(screen.getByText('已发布（派工记录尚未核实）')).toBeTruthy();
  view.rerender(<TaskDispatchStatus task={{ ...task, publication_epoch: undefined }} attempts={[]} />);
  expect(screen.getByText('已发布（发布版本未知，需核查）')).toBeTruthy();
});

it('keeps draft status separate from any stale dispatch evidence', () => {
  render(<TaskDispatchStatus task={{ ...task, status: 'draft' }} attempts={[attempt('in_flight')]} />);
  expect(screen.getByText('草案')).toBeTruthy();
});

it('prioritizes unknown evidence if inconsistent records are returned', () => {
  render(<TaskDispatchStatus task={task} attempts={[
    attempt('pending'), { ...attempt('outcome_unknown'), id: 'attempt-2' },
  ]} />);
  expect(screen.getByText('派工结果未知（需核查，不可盲目重试）')).toBeTruthy();
});

it('marks cached task evidence unverified when the latest snapshot read fails', () => {
  const view = render(<TaskDispatchStatus task={task} attempts={[attempt('pending')]} fresh={false} />);
  expect(screen.getByText('状态仅为上次读取结果（当前未核实）')).toBeTruthy();
  view.rerender(<TaskDispatchStatus task={task} attempts={[attempt('pending')]} fresh={true} />);
  expect(screen.getByText('已登记待派工（尚未执行）')).toBeTruthy();
});
