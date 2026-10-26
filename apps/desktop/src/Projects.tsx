import { MemberBriefPanel } from './MemberBriefPanel';
import { TaskJournal } from './TaskJournal';
import { TaskEditor } from './TaskEditor';
import { TaskDispatchControls } from './TaskDispatchControls';
import { TaskDispatchStatus } from './TaskDispatchStatus';
import { ObjectEditor } from './ObjectEditor';
import { useState } from 'react';
import { command } from './client';
import { createAgent, type OpenChat, type Run } from './App';
import { VerifiedConnectionCreation } from './Connections';
import { roleNames, type AgentRole, type Project, type Snapshot } from './types';

export function ProjectDetail({project,data,snapshotFresh,run,openChat}:{project:Project;data:Snapshot;snapshotFresh:boolean;run:Run;openChat:OpenChat}) {
  const [selectedTask,setSelectedTask]=useState('');
  const [briefAgent,setBriefAgent]=useState('');
  const agents=data.agents.filter(a=>a.project_id===project.id);
  // Task eligibility follows the Service's explicit project duty rule.
  // Keep the member workspace/chat list limited to its original project:
  // a duty assignment alone grants no cross-project data or tool access.
  const dutyAgentIds=new Set(data.agent_duties.filter(d=>d.project_id===project.id).map(d=>d.agent_id));
  const taskAgents=data.agents.filter(a=>a.project_id===project.id||dutyAgentIds.has(a.id));
  const tasks=data.tasks.filter(t=>t.project_id===project.id);
  const task=tasks.find(t=>t.id===selectedTask);
  // Preserve historical assignees in the editor even if their duty changed,
  // rather than silently deleting them on an unrelated task edit.
  const editorAgents=task?data.agents.filter(a=>taskAgents.some(eligible=>eligible.id===a.id)||task.agent_ids.includes(a.id)):taskAgents;
  const dutyLabels:Record<string,string>=Object.fromEntries(editorAgents.map(a=>{
    const duties=data.agent_duties.filter(d=>d.project_id===project.id&&d.agent_id===a.id)
      .map(d=>roleNames[d.duty_id as AgentRole]||d.duty_id);
    const eligible=taskAgents.some(member=>member.id===a.id);
    return [a.id,eligible?(duties.join(' / ')||roleNames[a.role]):'原有分配（当前无本项目职责，需核对）'];
  }));
  const canAdd=data.connections.some(c=>c.enabled);
  return <section key={project.id}><h1>{project.name}</h1><p className="prewrap">{project.goal||'尚未填写目标'}</p><fieldset disabled={!snapshotFresh} aria-label="项目资料编辑"><ObjectEditor key={project.id} kind="project" record={project} revision={data.revision} run={run}/></fieldset><details><summary>关联代码与资料</summary>{project.resources.length?project.resources.map((r,i)=><p key={i} className="path">{r}</p>):<p>尚未关联资料。</p>}<small>关联信息不授予访问权限。</small></details><hr/>
    <label>关联上下文<select disabled={!snapshotFresh} value={project.context_id||''} onChange={e=>{if(!snapshotFresh)return;const contextId=e.target.value||null;void run(async()=>{await command('bind_project_context',{projectId:project.id,contextId});});}}><option value="">暂不关联</option>{data.contexts.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label><small>关联用于组织与追溯，不复制上下文资料，也不授予工具访问权限。</small>
    <h2>AI 成员与分工</h2>{!agents.length&&<p>先添加主 AI，再按需添加施工、验证和审查 AI。每个成员拥有独立会话。</p>}
    {agents.map(a=><div key={a.id} className="item row"><div><strong>{a.name} · {roleNames[a.role]}</strong><p>{data.connections.find(c=>c.id===a.connection_id)?.name} / {a.model_id}</p></div><button onClick={()=>{const c=data.conversations.find(c=>c.id===a.conversation_id);if(c)openChat(c);}}>进入工作对话</button><button onClick={()=>setBriefAgent(a.id)}>查看工作资料</button></div>)}
    {agents.some(a=>a.id===briefAgent)&&<MemberBriefPanel key={briefAgent} agentId={briefAgent} revision={data.revision} close={()=>setBriefAgent('')}/>}
    <details><summary>关联已有成员的其他职责（试验性）</summary>
      <small>同一成员可兼任多个项目职责或用户级秘书；这只是职责登记，不授予项目数据、文件或工具访问权限，也不自动创建跨项目工作会话。</small>
      {data.agent_duties.filter(d=>d.project_id===project.id).map(d=><p key={d.id}>{data.agents.find(a=>a.id===d.agent_id)?.name||'未知成员'} · {d.duty_id}</p>)}
      <fieldset disabled={!snapshotFresh}><form onSubmit={e=>{e.preventDefault();if(!snapshotFresh)return;const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await command('assign_agent_duty',{agentId:String(f.get('agent')),projectId:f.get('scope')==='user'?null:project.id,dutyId:String(f.get('duty'))});form.reset();});}}>
        <label>已有成员<select name="agent" required>{data.agents.map(a=><option key={a.id} value={a.id}>{a.name}（创建于 {data.projects.find(p=>p.id===a.project_id)?.name||'未知项目'}）</option>)}</select></label>
        <label>职责<select name="duty"><option value="lead">项目主 AI</option><option value="worker">施工 AI</option><option value="reviewer">审查 AI</option><option value="verifier">验证 AI</option><option value="secretary">秘书</option></select></label>
        <label>登记范围<select name="scope"><option value="project">本项目</option><option value="user">用户级（独立于项目）</option></select></label>
        <button disabled={!data.agents.length}>登记职责</button>
      </form></fieldset>
    </details>
    <details><summary>添加 AI 成员</summary><form onSubmit={e=>{e.preventDefault();if(!snapshotFresh||!canAdd)return;const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await createAgent({project_id:project.id,name:String(f.get('name')),role:f.get('role') as AgentRole,connection_id:String(f.get('connection')),model_id:String(f.get('model'))});form.reset();});}}><div className="fields"><label>成员名称<input name="name" required maxLength={128}/></label><label>职责<select name="role">{Object.entries(roleNames).map(([id,name])=><option key={id} value={id}>{name}</option>)}</select></label></div><VerifiedConnectionCreation connections={data.connections} snapshotFresh={snapshotFresh} label="添加成员"/></form></details><hr/>
    <h2>任务卡</h2>{!snapshotFresh&&<p className="notice" role="status">项目与任务资料来自上次成功快照，当前未核实；项目编辑、职责登记、任务修改、手动记录、发布与派工均已暂停。</p>}<p className="muted">可保存任务草案并发布已具备成员、范围和验收条件的任务；发布只登记待派工事项，不启动施工或授予权限。</p>
    {tasks.map(t=><button className="item" key={t.id} onClick={()=>setSelectedTask(t.id)}><strong>{t.title}</strong><span><TaskDispatchStatus task={t} attempts={snapshotFresh?data.dispatch_attempts:undefined}/> · {t.agent_ids.length} 位成员</span></button>)}
    {task&&<article className="task-detail"><div className="row"><h3>{task.title}</h3><button onClick={()=>setSelectedTask('')}>收起</button></div>{task.status==='draft'?<><fieldset disabled={!snapshotFresh} aria-label="任务草案编辑"><TaskEditor key={task.id} task={task} agents={editorAgents} dutyLabels={dutyLabels} revision={data.revision} run={run}/></fieldset><button type="button" disabled={!snapshotFresh||!task.agent_ids.length||!task.scope.length||!task.acceptance.trim()} onClick={()=>void run(async()=>{await command('publish_task',{revision:data.revision,taskId:task.id});})}>发布任务（不启动施工）</button></>:<div><p>任务已发布；编辑请通过后续任务修订流程，尚未实际派发或执行。</p>{snapshotFresh?<TaskDispatchControls key={task.id} task={task} agents={data.agents} revision={data.revision} attempts={data.dispatch_attempts} run={run}/>:<p className="muted">当前快照未核实，已暂停准入预检和派工登记；请先重新读取。</p>}</div>}<h4>目标</h4><p className="prewrap">{task.goal||'未填写'}</p><h4>修改范围</h4><p className="prewrap">{task.scope.join('\n')||'未填写'}</p><h4>验收条件</h4><p className="prewrap">{task.acceptance||'未填写'}</p><h4>参与成员</h4><p>{task.agent_ids.map(id=>data.agents.find(a=>a.id===id)?.name||`未知成员（${id}）`).join('、')||'未分配'}</p><p>施工、实际验证、审查与修复流转尚未接通。</p><fieldset disabled={!snapshotFresh} aria-label="任务记录编辑"><TaskJournal key={task.id} task={task} records={data.task_records.filter(r=>r.task_id===task.id)} revision={data.revision} run={run}/></fieldset></article>}
    <details><summary>新建任务草案</summary><fieldset disabled={!snapshotFresh}><form onSubmit={e=>{e.preventDefault();if(!snapshotFresh)return;const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await command('create_task',{input:{project_id:project.id,title:f.get('title'),goal:f.get('goal'),scope:String(f.get('scope')||'').split('\n').filter(Boolean),acceptance:f.get('acceptance'),agent_ids:f.getAll('agents')}});form.reset();});}}><label>任务名称<input name="title" required maxLength={128}/></label><label>具体目标<textarea name="goal" rows={3}/></label><label>修改范围<textarea name="scope" rows={2} placeholder="每行一项，供后续并发范围控制使用"/></label><label>验收条件<textarea name="acceptance" rows={3}/></label><fieldset><legend>分配成员</legend>{taskAgents.map(a=><label className="check" key={a.id}><input type="checkbox" name="agents" value={a.id}/>{a.name} · {dutyLabels[a.id]}</label>)}{!taskAgents.length&&<small>可先保存草案，稍后添加成员。</small>}<small>跨项目职责只允许登记任务分配，不自动授予项目资料、会话或工具访问权限。</small></fieldset><button className="primary">保存草案</button></form></fieldset></details>
  </section>;
}
