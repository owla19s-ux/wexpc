import { useState } from 'react';
import type { Run } from './App';
import { command, errorMessage } from './client';
import type { Snapshot, Task, TaskRecord, TaskRecordKind } from './types';

const labels:Record<TaskRecordKind,string>={progress:'进展',blocker:'阻碍',handoff:'交接'};

type ReloadedTask = {revision:number;task:Task};

export function TaskJournal({task,records,revision,run}:{task:Task;records:TaskRecord[];revision:number;run:Run}) {
  const [editing,setEditing]=useState(false);
  const [base,setBase]=useState(revision);
  const [saving,setSaving]=useState(false);
  const [reloading,setReloading]=useState(false);
  const [reloaded,setReloaded]=useState<ReloadedTask|null>(null);
  const [reloadConfirmed,setReloadConfirmed]=useState(false);
  const [reloadError,setReloadError]=useState('');
  const reload=async()=>{
    if(saving||reloading)return;
    setReloading(true);
    setReloadError('');
    setReloadConfirmed(false);
    try {
      const latest=await command<Snapshot>('snapshot');
      const current=latest.tasks.find(t=>t.id===task.id);
      if(!current)throw new Error('当前任务已不存在，无法继续追加记录。');
      setReloaded({revision:latest.revision,task:current});
    }catch(e){
      setReloaded(null);
      setReloadError(errorMessage(e));
    }finally{setReloading(false);}
  };
  return <section aria-label="任务记录"><h4>进展、阻碍与交接</h4>
    <p className="muted">以下为手动记录，引用未自动核验，不代表工具执行、测试或审查通过。</p>
    {!records.length&&<p>暂无记录。可以先记下进展和下一步，稍后接续。</p>}
    {records.map(r=><article className="turn" key={r.id}>
      <strong>{labels[r.kind]}</strong><small> · 手动记录 · 记录序号 {r.recorded_revision}</small>
      <p className="prewrap">{r.body}</p>
      {!!r.references.length&&<><h4>引用（未自动核验）</h4>{r.references.map((ref,i)=><p className="path" key={i}>{ref}</p>)}</>}
      <details><summary>记录时的任务内容</summary><p>{r.task_snapshot.title}</p><h4>目标</h4><p className="prewrap">{r.task_snapshot.goal||'未填写'}</p><h4>修改范围</h4><p className="prewrap">{r.task_snapshot.scope.join('\n')||'未填写'}</p><h4>验收条件</h4><p className="prewrap">{r.task_snapshot.acceptance||'未填写'}</p></details>
    </article>)}
    {!editing?<button onClick={()=>{setBase(revision);setReloaded(null);setReloadConfirmed(false);setReloadError('');setEditing(true);}}>添加进展或交接</button>:
    <form onSubmit={e=>{
      e.preventDefault();if(saving||reloading||reloadError||(reloaded&&!reloadConfirmed))return;
      const f=new FormData(e.currentTarget);
      const body=String(f.get('body')||'');
      setSaving(true);
      void run(async()=>{try {
        await command('append_task_record',{revision:base,input:{task_id:task.id,kind:f.get('kind'),body,references:String(f.get('references')||'').split('\n').map(v=>v.trim()).filter(Boolean)}});
        setEditing(false);
      }finally{setSaving(false);}});
    }}><fieldset disabled={saving||reloading}>
      <label>记录类型<select name="kind">{Object.entries(labels).map(([key,label])=><option key={key} value={key}>{label}</option>)}</select></label>
      <label>内容<textarea name="body" required rows={4} placeholder="已做什么、遇到什么阻碍、下一步需要谁接续"/></label>
      <label>版本与资料引用<textarea name="references" rows={2} placeholder="每行一项，例如 Commit、Run 地址或日志路径"/></label>
      <p className="muted">保存会保留当前任务内容作为历史快照。记录只追加，不更改任务状态或启动施工；后续可另加更正说明。</p>
      <p className="muted">若任务资料已变化，保存会拒绝覆盖并保留输入。重新读取后请核对最新任务，再确认使用该版本；不会清空当前填写内容。</p>
      {reloadError&&<p className="error" role="alert">重新读取失败：{reloadError}</p>}
      {reloaded&&<div className="notice" role="status">
        <p>已读取任务最新快照（版本 {reloaded.revision}；本次编辑原基准 {base}）。请核对以下内容，确认后才能保存记录。</p>
        <p><strong>任务：</strong>{reloaded.task.title}</p>
        <p className="prewrap"><strong>目标：</strong>{reloaded.task.goal||'未填写'}</p>
        <p className="prewrap"><strong>修改范围：</strong>{reloaded.task.scope.join('\n')||'未填写'}</p>
        <p className="prewrap"><strong>验收条件：</strong>{reloaded.task.acceptance||'未填写'}</p>
        <p>分配成员：{reloaded.task.agent_ids.join('、')||'无'}；状态：{reloaded.task.status}</p>
        {!reloadConfirmed?<button type="button" onClick={()=>{setBase(reloaded.revision);setReloadConfirmed(true);}}>已核对，使用最新版本</button>:<p>已确认当前版本。若其他操作再次修改数据，服务端仍会拒绝过期修订。</p>}
      </div>}
      <div className="row"><button className="primary" disabled={Boolean(reloadError||(reloaded&&!reloadConfirmed))}>{saving?'保存中…':'保存记录'}</button><button type="button" onClick={()=>setEditing(false)}>收起填写</button><button type="button" onClick={()=>void reload()}>{reloading?'正在读取…':'重新读取数据'}</button></div>
    </fieldset></form>}
  </section>;
}
