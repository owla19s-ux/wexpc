import { MemberBriefPanel } from './MemberBriefPanel';
import { TaskJournal } from './TaskJournal';
import { TaskEditor } from './TaskEditor';
import { ObjectEditor } from './ObjectEditor';
import { useState } from 'react';
import { command } from './client';
import { createAgent, type OpenChat, type Run } from './App';
import { Selection } from './Connections';
import { roleNames, type AgentRole, type Project, type Snapshot } from './types';

export function ProjectDetail({project,data,run,openChat}:{project:Project;data:Snapshot;run:Run;openChat:OpenChat}) {
  const [selectedTask,setSelectedTask]=useState('');
  const [briefAgent,setBriefAgent]=useState('');
  const agents=data.agents.filter(a=>a.project_id===project.id);
  const tasks=data.tasks.filter(t=>t.project_id===project.id);
  const task=tasks.find(t=>t.id===selectedTask);
  const canAdd=data.connections.some(c=>c.enabled);
  return <section key={project.id}><h1>{project.name}</h1><p className="prewrap">{project.goal||'尚未填写目标'}</p><ObjectEditor key={project.id} kind="project" record={project} revision={data.revision} run={run}/><details><summary>关联代码与资料</summary>{project.resources.length?project.resources.map((r,i)=><p key={i} className="path">{r}</p>):<p>尚未关联资料。</p>}<small>关联信息不授予访问权限。</small></details><hr/>
    <label>关联上下文<select value={project.context_id||''} onChange={e=>{const contextId=e.target.value||null;void run(async()=>{await command('bind_project_context',{projectId:project.id,contextId});});}}><option value="">暂不关联</option>{data.contexts.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label><small>关联用于组织与追溯，不复制上下文资料，也不授予工具访问权限。</small>
    <h2>AI 成员与分工</h2>{!agents.length&&<p>先添加主 AI，再按需添加施工、验证和审查 AI。每个成员拥有独立会话。</p>}
    {agents.map(a=><div key={a.id} className="item row"><div><strong>{a.name} · {roleNames[a.role]}</strong><p>{data.connections.find(c=>c.id===a.connection_id)?.name} / {a.model_id}</p></div><button onClick={()=>{const c=data.conversations.find(c=>c.id===a.conversation_id);if(c)openChat(c);}}>进入工作对话</button><button onClick={()=>setBriefAgent(a.id)}>查看工作资料</button></div>)}
    {agents.some(a=>a.id===briefAgent)&&<MemberBriefPanel key={briefAgent} agentId={briefAgent} revision={data.revision} close={()=>setBriefAgent('')}/>}
    <details><summary>添加 AI 成员</summary><form onSubmit={e=>{e.preventDefault();const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await createAgent({project_id:project.id,name:String(f.get('name')),role:f.get('role') as AgentRole,connection_id:String(f.get('connection')),model_id:String(f.get('model'))});form.reset();});}}><div className="fields"><label>成员名称<input name="name" required maxLength={128}/></label><label>职责<select name="role">{Object.entries(roleNames).map(([id,name])=><option key={id} value={id}>{name}</option>)}</select></label></div><Selection connections={data.connections}/><button className="primary" disabled={!canAdd}>添加成员</button>{!canAdd&&<p>请先在连接与服务中配置 API。</p>}</form></details><hr/>
    <h2>任务卡</h2><p className="muted">本批可记录目标、范围、成员与验收条件；任务保持草案，尚不启动施工。</p>
    {tasks.map(t=><button className="item" key={t.id} onClick={()=>setSelectedTask(t.id)}><strong>{t.title}</strong><span>草案 · {t.agent_ids.length} 位成员</span></button>)}
    {task&&<article className="task-detail"><div className="row"><h3>{task.title}</h3><button onClick={()=>setSelectedTask('')}>收起</button></div><TaskEditor key={task.id} task={task} agents={agents} revision={data.revision} run={run}/><h4>目标</h4><p className="prewrap">{task.goal||'未填写'}</p><h4>修改范围</h4><p className="prewrap">{task.scope.join('\n')||'未填写'}</p><h4>验收条件</h4><p className="prewrap">{task.acceptance||'未填写'}</p><h4>参与成员</h4><p>{task.agent_ids.map(id=>agents.find(a=>a.id===id)?.name).join('、')||'未分配'}</p><p>施工、实际验证、审查与修复流转尚未接通。</p><TaskJournal key={task.id} task={task} records={data.task_records.filter(r=>r.task_id===task.id)} revision={data.revision} run={run}/></article>}
    <details><summary>新建任务草案</summary><form onSubmit={e=>{e.preventDefault();const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await command('create_task',{input:{project_id:project.id,title:f.get('title'),goal:f.get('goal'),scope:String(f.get('scope')||'').split('\n').filter(Boolean),acceptance:f.get('acceptance'),agent_ids:f.getAll('agents')}});form.reset();});}}><label>任务名称<input name="title" required maxLength={128}/></label><label>具体目标<textarea name="goal" rows={3}/></label><label>修改范围<textarea name="scope" rows={2} placeholder="每行一项，供后续并发范围控制使用"/></label><label>验收条件<textarea name="acceptance" rows={3}/></label><fieldset><legend>分配成员</legend>{agents.map(a=><label className="check" key={a.id}><input type="checkbox" name="agents" value={a.id}/>{a.name} · {roleNames[a.role]}</label>)}{!agents.length&&<small>可先保存草案，稍后添加成员。</small>}</fieldset><button className="primary">保存草案</button></form></details>
  </section>;
}
