import type { DispatchAttempt, Task } from './types';

export function taskDispatchLabel(task: Task, attempts?: DispatchAttempt[]): string {
  if (task.status === 'draft') return '草案';
  // Old snapshots and interrupted reads must not be presented as "not dispatched".
  if (task.publication_epoch == null) return '已发布（发布版本未知，需核查）';
  if (!attempts) return '已发布（派工记录尚未核实）';

  const current = attempts.filter(attempt =>
    attempt.project_id === task.project_id &&
    attempt.task_id === task.id &&
    attempt.publication_epoch === task.publication_epoch
  );
  // Fail closed when evidence is contradictory: never hide UNKNOWN behind Pending.
  if (current.some(attempt => attempt.status === 'outcome_unknown'))
    return '派工结果未知（需核查，不可盲目重试）';
  if (current.some(attempt => attempt.status === 'in_flight'))
    return '已领取（执行结果未确认）';
  if (current.some(attempt => attempt.status === 'pending'))
    return '已登记待派工（尚未执行）';
  if (current.length) return '派工状态未知（需核查）';
  return '已发布（尚未登记派工）';
}

export function TaskDispatchStatus({ task, attempts }: {
  task: Task;
  attempts?: DispatchAttempt[];
}) {
  return <>{taskDispatchLabel(task, attempts)}</>;
}
