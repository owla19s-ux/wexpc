import { expect, it } from 'vitest';
import { taskFooterSummary } from './TaskOverview';
import type { DispatchAttempt, Snapshot, Task } from './types';

const published: Task = {
  id: 'task-1', project_id: 'project-a', title: '任务', goal: '',
  scope: ['src'], acceptance: '通过测试', agent_ids: ['agent-a'],
  status: 'published', publication_epoch: 4,
};
const attempt = (status: DispatchAttempt['status'], epoch = 4): DispatchAttempt => ({
  id: 'attempt-1', project_id: 'project-a', task_id: 'task-1',
  agent_id: 'agent-a', publication_epoch: epoch, status, recorded_revision: 7,
});
const state = (tasks: Task[], dispatch_attempts?: DispatchAttempt[]):
  Pick<Snapshot, 'tasks' | 'dispatch_attempts'> => ({ tasks, dispatch_attempts });

it('does not report drafts before snapshot load or when there are no tasks', () => {
  expect(taskFooterSummary(null)).toBe('任务状态尚未加载');
  expect(taskFooterSummary(state([]))).toBe('暂无任务');
});

it('reports drafts without implying that they have been published', () => {
  const text = taskFooterSummary(state([{ ...published, status: 'draft' }]));
  expect(text).toContain('1 个草案');
  expect(text).not.toContain('已发布');
});

it('does not mislabel published tasks as drafts when dispatch evidence is missing', () => {
  const text = taskFooterSummary(state([published]));
  expect(text).toContain('1 个已发布');
  expect(text).toContain('1 个派工证据未核实');
  expect(text).not.toContain('尚未登记派工');
  expect(text).toContain('发布/登记不代表实际施工');
});

it('distinguishes unqueued, pending, and claimed without claiming execution', () => {
  expect(taskFooterSummary(state([published], []))).toContain('1 个尚未登记派工');
  expect(taskFooterSummary(state([published], [attempt('pending')]))).toContain('1 个待领取');
  const claimed = taskFooterSummary(state([published], [attempt('in_flight')]));
  expect(claimed).toContain('1 个已领取（结果未确认）');
  expect(claimed).not.toContain('执行成功');
});

it('gives unknown outcomes precedence over contradictory pending evidence', () => {
  const text = taskFooterSummary(state([published], [
    attempt('pending'), { ...attempt('outcome_unknown'), id: 'attempt-2' },
  ]));
  expect(text).toContain('1 个结果未知（需核查，不可盲目重试）');
  expect(text).not.toContain('待领取');
});

it('does not project historical or cross-project attempts into the current publication', () => {
  const text = taskFooterSummary(state([published], [
    attempt('outcome_unknown', 3),
    { ...attempt('in_flight'), project_id: 'project-b' },
  ]));
  expect(text).toContain('1 个尚未登记派工');
  expect(text).not.toContain('结果未知');
});
