import { roleNames, type Agent, type AgentDutyAssignment } from './types';

const dutyNames: Record<string, string> = {
  lead: '主 AI', worker: '施工 AI', reviewer: '审查 AI',
  verifier: '验证 AI', secretary: '秘书',
};

/** Task membership follows project duties, not an agent's creation project. */
export function taskEligibleAgents(agents: Agent[], duties: AgentDutyAssignment[], projectId: string): Agent[] {
  const assigned = new Set(duties.filter(d => d.project_id === projectId).map(d => d.agent_id));
  return agents.filter(a => a.project_id === projectId || assigned.has(a.id));
}

/** Display the duty in the target project; fall back for legacy home-project agents. */
export function taskDutyLabel(agent: Agent, duties: AgentDutyAssignment[], projectId: string): string {
  const labels = duties.filter(d => d.agent_id === agent.id && d.project_id === projectId)
    .map(d => dutyNames[d.duty_id] ?? d.duty_id);
  return labels.length ? labels.join('、') : roleNames[agent.role];
}
