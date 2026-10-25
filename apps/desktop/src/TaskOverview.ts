import type { DispatchAttempt, Snapshot } from './types';

// This is a read-only view of persisted task/dispatch facts, not a dispatcher.
// In particular, "claimed" is not evidence that a tool ran or a task passed review.
export function taskFooterSummary(data: Pick<Snapshot, 'tasks' | 'dispatch_attempts'> | null): string {
  if (!data) return '任务状态尚未加载';
  if (!data.tasks.length) return '暂无任务';

  const drafts = data.tasks.filter(task => task.status === 'draft').length;
  const published = data.tasks.filter(task => task.status === 'published');
  const counts = { unverified: 0, unqueued: 0, pending: 0, claimed: 0, unknown: 0 };

  for (const task of published) {
    if (task.publication_epoch == null || !data.dispatch_attempts) {
      counts.unverified++;
      continue;
    }
    const current = data.dispatch_attempts.filter((attempt: DispatchAttempt) =>
      attempt.project_id === task.project_id &&
      attempt.task_id === task.id &&
      attempt.publication_epoch === task.publication_epoch
    );
    // Contradictory records must never conceal an unknown external outcome.
    if (current.some(attempt => attempt.status === 'outcome_unknown')) counts.unknown++;
    else if (current.some(attempt => attempt.status === 'in_flight')) counts.claimed++;
    else if (current.some(attempt => attempt.status === 'pending')) counts.pending++;
    else if (current.length) counts.unverified++;
    else counts.unqueued++;
  }

  const taskParts = [
    drafts ? `${drafts} 个草案` : '',
    published.length ? `${published.length} 个已发布` : '',
    data.tasks.length !== drafts + published.length ? '部分任务状态需核查' : '',
  ].filter(Boolean);
  const evidenceParts = [
    counts.unknown ? `${counts.unknown} 个结果未知（需核查，不可盲目重试）` : '',
    counts.claimed ? `${counts.claimed} 个已领取（结果未确认）` : '',
    counts.pending ? `${counts.pending} 个待领取` : '',
    counts.unqueued ? `${counts.unqueued} 个尚未登记派工` : '',
    counts.unverified ? `${counts.unverified} 个派工证据未核实` : '',
  ].filter(Boolean);

  return `任务：${taskParts.join('、')}${evidenceParts.length ? '；' + evidenceParts.join('、') : ''}${published.length ? '；发布/登记不代表实际施工' : ''}`;
}
