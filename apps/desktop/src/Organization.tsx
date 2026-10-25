import { ObjectEditor } from './ObjectEditor';
import { useState } from 'react';
import { command } from './client';
import type { Snapshot } from './types';
import type { Run } from './App';

export function Organization({data,run}:{data:Snapshot;run:Run}) {
  const [spaceId,setSpaceId]=useState('');
  const selected=data.spaces.find(s=>s.id===spaceId)||data.spaces[0];
  const groups=data.groups.filter(g=>g.space_id===selected?.id);
  const contexts=data.contexts.filter(c=>data.placements.some(p=>p.context_id===c.id&&p.space_id===selected?.id));
  return <section><h1>Space 与上下文</h1><p>空间和分组用于组织工作，移动不会授予访问权限或移动外部文件。</p>
    <form onSubmit={e=>{e.preventDefault();const f=e.currentTarget;const name=new FormData(f).get('name');void run(async()=>{await command('create_space',{name});f.reset();});}}><label>新空间名称<input name="name" required maxLength={128}/></label><button>创建空间</button></form>
    {!!data.spaces.length&&<><label>当前空间<select value={selected?.id||''} onChange={e=>setSpaceId(e.target.value)}>{data.spaces.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
    {selected&&<ObjectEditor key={selected.id} kind="space" record={selected} revision={data.revision} run={run}/>}
    <details><summary>管理已有分组</summary>{groups.map(g=><div className="item" key={g.id}><strong>{g.name}</strong><ObjectEditor kind="group" record={g} revision={data.revision} run={run}/></div>)}</details>
    <details><summary>创建分组</summary><form onSubmit={e=>{e.preventDefault();const f=e.currentTarget;const name=new FormData(f).get('name');void run(async()=>{await command('create_group',{spaceId:selected?.id,name});f.reset();});}}><label>分组名称<input name="name" required maxLength={128}/></label><button>创建分组</button></form></details>
    <details><summary>新建上下文</summary><form key={selected?.id} onSubmit={e=>{e.preventDefault();const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await command('create_context',{name:f.get('name'),goal:f.get('goal'),resources:String(f.get('resources')||'').split('\n').map(s=>s.trim()).filter(Boolean),spaceId:selected?.id,groupId:f.get('group')||null});form.reset();});}}><label>名称<input name="name" required maxLength={128}/></label><label>目标与背景<textarea name="goal" rows={3}/></label><label>资料地址<textarea name="resources" rows={2}/></label><label>分组<select name="group"><option value="">未分组</option>{groups.map(g=><option key={g.id} value={g.id}>{g.name}</option>)}</select></label><button>保存上下文</button></form></details>
    <h2>上下文</h2>{!contexts.length&&<p>这个空间还没有上下文。可以整理设计、资料或其他工作，不要求先创建代码项目。</p>}{contexts.map(c=>{const location=data.placements.find(p=>p.context_id===c.id);return <article key={c.id} className="item"><strong>{c.name}</strong><small>{groups.find(g=>g.id===location?.group_id)?.name||'未分组'}</small><p className="prewrap">{c.goal}</p>{c.resources.map((r,i)=><span key={i}>{r}</span>)}<ObjectEditor kind="context" record={c} revision={data.revision} run={run}/><details><summary>移动组织位置</summary><MoveContext key={c.id+location?.space_id+location?.group_id} contextId={c.id} data={data} initialSpace={location?.space_id||''} initialGroup={location?.group_id||''} run={run}/></details></article>})}</>}
  </section>;
}

function MoveContext({contextId,data,initialSpace,initialGroup,run}:{contextId:string;data:Snapshot;initialSpace:string;initialGroup:string;run:Run}) {
  const [space,setSpace]=useState(initialSpace);
  return <form onSubmit={e=>{e.preventDefault();const groupId=new FormData(e.currentTarget).get('group')||null;void run(async()=>{await command('move_context',{contextId,spaceId:space,groupId});});}}><label>目标空间<select value={space} onChange={e=>setSpace(e.target.value)}>{data.spaces.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label><label>目标分组<select key={space} name="group" defaultValue={space===initialSpace?initialGroup:''}><option value="">未分组</option>{data.groups.filter(g=>g.space_id===space).map(g=><option key={g.id} value={g.id}>{g.name}</option>)}</select></label><button>保存位置</button></form>;
}
