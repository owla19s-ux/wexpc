import { useState, type FormEvent } from 'react';
import { command, errorMessage } from './client';
import type { Run } from './App';
import { roleNames, type Agent, type AgentRole, type Snapshot, type Task } from './types';

type ReloadReview = {
  revision: number;
  task: Task;
  agents: Agent[];
  dutyLabels: Record<string,string>;
};

export function reviewTask(snapshot:Snapshot, taskId:string):ReloadReview {
  const task=snapshot.tasks.find(t=>t.id===taskId);
  if(!task)throw new Error('任务已不存在，不能继续保存旧任务。');
  const dutyIds=new Set(snapshot.agent_duties.filter(d=>d.project_id===task.project_id).map(d=>d.agent_id));
  const eligible=new Set(snapshot.agents.filter(a=>a.project_id===task.project_id||dutyIds.has(a.id)).map(a=>a.id));
  const agents=snapshot.agents.filter(a=>eligible.has(a.id)||task.agent_ids.includes(a.id));
  const dutyLabels:Record<string,string>={};
  for(const agent of agents){
    const duties=snapshot.agent_duties.filter(d=>d.project_id===task.project_id&&d.agent_id===agent.id)
      .map(d=>roleNames[d.duty_id as AgentRole]||d.duty_id);
    dutyLabels[agent.id]=eligible.has(agent.id)
      ?(duties.join(' / ')||roleNames[agent.role])
      :'原有分配（当前无本项目职责，需核对）';
  }
  return {revision:snapshot.revision,task,agents,dutyLabels};
}

export function TaskEditor({task,agents,dutyLabels,revision,run}:{task:Task;agents:Agent[];dutyLabels:Record<string,string>;revision:number;run:Run}) {
  const [editing,setEditing]=useState(false);
  const [base,setBase]=useState(revision);
  const [saving,setSaving]=useState(false);
  const [reloading,setReloading]=useState(false);
  const [review,setReview]=useState<ReloadReview|null>(null);
  const [reviewConfirmed,setReviewConfirmed]=useState(false);
  const [reloadError,setReloadError]=useState('');
  const [working,setWorking]=useState<ReloadReview|null>(null);
  const [formVersion,setFormVersion]=useState(0);

  const reload=async()=>{
    if(saving||reloading)return;
    setReloading(true);
    setReloadError('');
    setReviewConfirmed(false);
    try {
      const snapshot=await command<Snapshot>('snapshot');
      setReview(reviewTask(snapshot,task.id));
    }catch(e){
      setReview(null);
      setReloadError(errorMessage(e));
    }finally{setReloading(false);}
  };
  if(!editing)return <button onClick={()=>{
    setBase(revision);setWorking(null);setReview(null);setReviewConfirmed(false);
    setReloadError('');setEditing(true);
  }}>编辑任务与分配</button>;

  const current=working?.task||task;
  const currentAgents=working?.agents||agents;
  const currentLabels=working?.dutyLabels||dutyLabels;
  return <form key={formVersion} onSubmit={(e:FormEvent<HTMLFormElement>)=>{
    e.preventDefault();
    if(saving||reloading||reloadError||(review&&!reviewConfirmed))return;
    const f=new FormData(e.currentTarget);
    setSaving(true);
    void run(async()=>{
      try {await command('update_task',{revision:base,taskId:task.id,input:{
        project_id:current.project_id,title:f.get('title'),goal:f.get('goal'),
        scope:String(f.get('scope')||'').split('\n').map(v=>v.trim()).filter(Boolean),
        acceptance:f.get('acceptance'),agent_ids:f.getAll('agents'),
      }});
      setEditing(false);
      }finally{setSaving(false);}
    });
  }}><fieldset disabled={saving||reloading}>
    <label>任务名称<input name="title" required maxLength={128} defaultValue={current.title}/></label>
    <label>具体目标<textarea name="goal" rows={3} defaultValue={current.goal}/></label>
    <label>修改范围<textarea name="scope" rows={3} defaultValue={current.scope.join('\n')}/></label>
    <label>验收条件<textarea name="acceptance" rows={3} defaultValue={current.acceptance}/></label>
    <fieldset><legend>分配成员</legend>{currentAgents.map(a=><label className="check" key={a.id}><input type="checkbox" name="agents" value={a.id} defaultChecked={current.agent_ids.includes(a.id)}/>{a.name} · {currentLabels[a.id]||roleNames[a.role]}</label>)}{!currentAgents.length&&<small>可先保存目标，再添加成员并分配。</small>}{currentAgents.some(a=>current.agent_ids.includes(a.id)&&currentLabels[a.id]?.startsWith('原有分配'))&&<small>原有成员已无本项目职责；保存前请移除该分配或先恢复职责，系统不会自动清除。</small>}</fieldset>
    <p className="muted">保存只更新草案，不启动施工。发生修订冲突时，可重新读取并核对最新任务；确认后会重载最新表单，未保存的编辑不会自动覆盖他人的修改。</p>
    {reloadError&&<p className="error" role="alert">重新读取失败：{reloadError}</p>}
    {review&&<div className="notice" role="status">
      <p>已读取最新任务（版本 {review.revision}；当前编辑基准 {base}）。确认前请复制需要保留的未提交修改；确认后表单会恢复为最新任务内容，可重新编辑。</p>
      <p><strong>任务：</strong>{review.task.title}</p>
      <p className="prewrap"><strong>目标：</strong>{review.task.goal||'未填写'}</p>
      <p className="prewrap"><strong>修改范围：</strong>{review.task.scope.join('\n')||'未填写'}</p>
      <p className="prewrap"><strong>验收条件：</strong>{review.task.acceptance||'未填写'}</p>
      <p>分配成员：{review.task.agent_ids.map(id=>review.agents.find(a=>a.id===id)?.name||id).join('、')||'无'}；状态：{review.task.status}</p>
      {!reviewConfirmed?<button type="button" onClick={()=>{
        setBase(review.revision);setWorking(review);setReviewConfirmed(true);
        setFormVersion(v=>v+1);
      }}>已核对，载入最新任务</button>:<p>已载入最新任务；如数据再次变化，服务端仍会拒绝过期修订。</p>}
    </div>}
    <div className="row"><button className="primary" disabled={Boolean(reloadError||(review&&!reviewConfirmed))}>{saving?'保存中…':'保存任务'}</button><button type="button" onClick={()=>setEditing(false)}>收起编辑</button><button type="button" onClick={()=>void reload()}>{reloading?'正在读取…':'重新读取数据'}</button></div>
  </fieldset></form>;
}
