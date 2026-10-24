export type AgentRole = 'lead' | 'worker' | 'verifier' | 'reviewer';
export interface AiConnection { id:string; name:string; base_url:string; models:string[]; enabled:boolean }
export interface Project { id:string; name:string; goal:string; resources:string[]; context_id:string|null }
export interface Space {id:string;name:string}
export interface Group {id:string;space_id:string;name:string}
export interface Context {id:string;name:string;goal:string;resources:string[]}
export interface Placement {context_id:string;space_id:string;group_id:string|null}
export interface Agent { id:string; project_id:string; name:string; role:AgentRole; connection_id:string; model_id:string; conversation_id:string }
export interface Task { id:string; project_id:string; title:string; goal:string; scope:string[]; acceptance:string; agent_ids:string[]; status:'draft' }
export type TaskRecordKind = 'progress'|'blocker'|'handoff';
export interface TaskRecord {id:string;task_id:string;kind:TaskRecordKind;source:'user';body:string;references:string[];task_snapshot:Task;recorded_revision:number}
export interface Conversation { context_id:string|null; id:string; project_id:string|null; title:string; connection_id:string; model_id:string }
export interface Turn { id:string; conversation_id:string; user:string; assistant:string|null; status:'running'|'succeeded'|'failed'|'interrupted'; error:string|null }
export interface Snapshot { revision:number; spaces:Space[];groups:Group[];contexts:Context[];placements:Placement[];connections:AiConnection[]; projects:Project[]; agents:Agent[]; tasks:Task[]; task_records:TaskRecord[]; conversations:Conversation[]; turns:Turn[]; unsaved_turn_ids:string[] }
export const roleNames:Record<AgentRole,string> = {lead:'主 AI',worker:'施工 AI',verifier:'验证 AI',reviewer:'审查 AI'};
