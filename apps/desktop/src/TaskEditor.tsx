import { useState } from 'react';
import { command } from './client';
import type { Run } from './App';
import { roleNames, type Agent, type Task } from './types';

export function TaskEditor({task,agents,dutyLabels,revision,run}:{task:Task;agents:Agent[];dutyLabels:Record<string,string>;revision:number;run:Run}) {
  const [editing,setEditing]=useState(false);
  const [base,setBase]=useState(revision);
  const [saving,setSaving]=useState(false);
  if(!editing)return <button onClick={()=>{setBase(revision);setEditing(true);}}>编辑任务与分配</button>;
  return <form onSubmit={e=>{
    e.preventDefault();
    if(saving)return;
    const f=new FormData(e.currentTarget);
    setSaving(true);
    void run(async()=>{
      try { await command('update_task',{revision:base,taskId:task.id,input:{
        project_id:task.project_id,title:f.get('title'),goal:f.get('goal'),
        scope:String(f.get('scope')||'').split('\n').map(v=>v.trim()).filter(Boolean),
        acceptance:f.get('acceptance'),agent_ids:f.getAll('agents'),
      }});
      setEditing(false);
      } finally {setSaving(false);}
    });
  }}><fieldset disabled={saving}>
    <label>任务名称<input name="title" required maxLength={128} defaultValue={task.title}/></label>
    <label>具体目标<textarea name="goal" rows={3} defaultValue={task.goal}/></label>
    <label>修改范围<textarea name="scope" rows={3} defaultValue={task.scope.join('\n')}/></label>
    <label>验收条件<textarea name="acceptance" rows={3} defaultValue={task.acceptance}/></label>
    <fieldset><legend>分配成员</legend>{agents.map(a=><label className="check" key={a.id}><input type="checkbox" name="agents" value={a.id} defaultChecked={task.agent_ids.includes(a.id)}/>{a.name} · {dutyLabels[a.id]||roleNames[a.role]}</label>)}{!agents.length&&<small>可先保存目标，再添加成员并分配。</small>}{agents.some(a=>task.agent_ids.includes(a.id)&&dutyLabels[a.id]?.startsWith('原有分配'))&&<small>原有成员已无本项目职责；保存前请移除该分配或先恢复职责，系统不会自动清除。</small>}</fieldset>
    <p className="muted">保存只更新草案，不启动施工。若发生冲突，输入会保留；可复制内容后收起编辑，核对最新任务再重新编辑。</p>
    <div className="row"><button className="primary">{saving?'保存中…':'保存任务'}</button><button type="button" onClick={()=>setEditing(false)}>收起编辑</button><button type="button" onClick={()=>void run(async()=>{})}>重新读取数据</button></div>
  </fieldset></form>;
}
