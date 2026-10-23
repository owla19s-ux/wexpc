export type AgentRole = 'lead' | 'worker' | 'verifier' | 'reviewer';
export interface AiConnection { id:string; name:string; base_url:string; models:string[]; enabled:boolean }
export interface Project { id:string; name:string; goal:string; resources:string[] }
export interface Agent { id:string; project_id:string; name:string; role:AgentRole; connection_id:string; model_id:string; conversation_id:string }
export interface Task { id:string; project_id:string; title:string; goal:string; scope:string[]; acceptance:string; agent_ids:string[]; status:'draft' }
export interface Conversation { id:string; project_id:string|null; title:string; connection_id:string; model_id:string }
export interface Turn { id:string; conversation_id:string; user:string; assistant:string|null; status:'running'|'succeeded'|'failed'|'interrupted'; error:string|null }
export interface Snapshot { revision:number; connections:AiConnection[]; projects:Project[]; agents:Agent[]; tasks:Task[]; conversations:Conversation[]; turns:Turn[]; unsaved_turn_ids:string[] }
export const roleNames:Record<AgentRole,string> = {lead:'主 AI',worker:'施工 AI',verifier:'验证 AI',reviewer:'审查 AI'};
