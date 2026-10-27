import { ObjectEditor } from './ObjectEditor';
import { useRef, useState } from 'react';
import { command } from './client';
import type { Snapshot } from './types';
import type { Run } from './App';

export function Organization({data,run:runAction,snapshotFresh=true}:{data:Snapshot;run:Run;snapshotFresh?:boolean}) {
  const [activeMoves,setActiveMoves]=useState<Record<string,boolean>>({});
  const run:Run=async action=>{if(snapshotFresh)await runAction(action);};
  const [spaceId,setSpaceId]=useState('');
  const selected=data.spaces.find(s=>s.id===spaceId)||data.spaces[0];
  const groups=data.groups.filter(g=>g.space_id===selected?.id);
  const contexts=data.contexts.filter(c=>activeMoves[c.id]||data.placements.some(p=>p.context_id===c.id&&p.space_id===selected?.id));
  return <section><h1>Space 与上下文</h1><p>空间和分组用于组织工作，移动不会授予访问权限或移动外部文件。</p>
    <fieldset disabled={!snapshotFresh} aria-label="组织编辑"><form onSubmit={e=>{e.preventDefault();const f=e.currentTarget;const name=new FormData(f).get('name');void run(async()=>{await command('create_space',{name});f.reset();});}}><label>新空间名称<input name="name" required maxLength={128}/></label><button>创建空间</button></form>
    {!!data.spaces.length&&<><label>当前空间<select value={selected?.id||''} onChange={e=>setSpaceId(e.target.value)}>{data.spaces.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
    {selected&&<ObjectEditor key={selected.id} kind="space" record={selected} revision={data.revision} run={run}/>}
    <details><summary>管理已有分组</summary>{groups.map(g=><div className="item" key={g.id}><strong>{g.name}</strong><ObjectEditor kind="group" record={g} revision={data.revision} run={run}/></div>)}</details>
    <details><summary>创建分组</summary><form onSubmit={e=>{e.preventDefault();const f=e.currentTarget;const name=new FormData(f).get('name');void run(async()=>{await command('create_group',{spaceId:selected?.id,name});f.reset();});}}><label>分组名称<input name="name" required maxLength={128}/></label><button>创建分组</button></form></details>
    <details><summary>新建上下文</summary><form key={selected?.id} onSubmit={e=>{e.preventDefault();const form=e.currentTarget;const f=new FormData(form);void run(async()=>{await command('create_context',{name:f.get('name'),goal:f.get('goal'),resources:String(f.get('resources')||'').split('\n').map(s=>s.trim()).filter(Boolean),spaceId:selected?.id,groupId:f.get('group')||null});form.reset();});}}><label>名称<input name="name" required maxLength={128}/></label><label>目标与背景<textarea name="goal" rows={3}/></label><label>资料地址<textarea name="resources" rows={2}/></label><label>分组<select name="group"><option value="">未分组</option>{groups.map(g=><option key={g.id} value={g.id}>{g.name}</option>)}</select></label><button>保存上下文</button></form></details>
    <h2>上下文</h2>{!contexts.length&&<p>这个空间还没有上下文。可以整理设计、资料或其他工作，不要求先创建代码项目。</p>}{contexts.map(c=>{const location=data.placements.find(p=>p.context_id===c.id);return <article key={c.id} className="item"><strong>{c.name}</strong><small>{groups.find(g=>g.id===location?.group_id)?.name||'未分组'}</small><p className="prewrap">{c.goal}</p>{c.resources.map((r,i)=><span key={i}>{r}</span>)}<ObjectEditor kind="context" record={c} revision={data.revision} run={run}/><details><summary>移动组织位置</summary><MoveContext key={c.id} contextId={c.id} data={data} initialSpace={location?.space_id||''} initialGroup={location?.group_id||''} snapshotFresh={snapshotFresh} retain={value=>setActiveMoves(current=>({...current,[c.id]:value}))} run={run}/></details></article>})}</>}
    </fieldset>
  </section>;
}

type MoveDraft = { spaceId: string; groupId: string; revision: number };

function MoveContext({contextId,data,initialSpace,initialGroup,run,snapshotFresh,retain}:{
  contextId:string;data:Snapshot;initialSpace:string;initialGroup:string;run:Run;
  snapshotFresh:boolean;retain:(active:boolean)=>void;
}) {
  const [draft,setDraft]=useState<MoveDraft|null>(null);
  const [saving,setSaving]=useState(false);
  const inFlight=useRef(false);
  const space=draft?.spaceId??initialSpace;
  const group=draft?.groupId??initialGroup;
  function change(spaceId:string,groupId:string) {
    if(!snapshotFresh||inFlight.current)return;
    setDraft({spaceId,groupId,revision:draft?.revision??data.revision});
    retain(true);
  }
  function save() {
    if(!snapshotFresh||inFlight.current)return;
    const requested=draft??{spaceId:space,groupId:group,revision:data.revision};
    setDraft(requested);
    retain(true);
    inFlight.current=true;
    setSaving(true);
    void run(async()=>{
      await command('move_context',{
        revision:requested.revision,contextId,spaceId:requested.spaceId,groupId:requested.groupId||null,
      });
      setDraft(null);
      retain(false);
    }).finally(()=>{inFlight.current=false;setSaving(false);});
  }
  const missingSpace=space!==''&&!data.spaces.some(s=>s.id===space);
  const missingGroup=group!==''&&!data.groups.some(g=>g.id===group&&g.space_id===space);
  return <form onSubmit={event=>{event.preventDefault();save();}}>
    <label>目标空间<select disabled={!snapshotFresh||saving} value={space}
      onChange={event=>change(event.target.value,'')}>
      {data.spaces.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}
      {missingSpace&&<option value={space}>原空间已不可用（需核查）</option>}
    </select></label>
    <label>目标分组<select disabled={!snapshotFresh||saving} value={group}
      onChange={event=>change(space,event.target.value)}>
      <option value="">未分组</option>
      {data.groups.filter(g=>g.space_id===space).map(g=><option key={g.id} value={g.id}>{g.name}</option>)}
      {missingGroup&&<option value={group}>原分组已不可用（需核查）</option>}
    </select></label>
    <button disabled={!snapshotFresh||saving}>保存位置</button>
    {draft&&<><p role="status">位置选择尚未确认，已保留供核对。刷新不会自动重试，也不会更新原编辑版本。</p>
      <button type="button" disabled={!snapshotFresh||saving} onClick={()=>{
        if(!snapshotFresh||inFlight.current)return;
        setDraft(null);retain(false);
      }}>放弃修改，使用最新位置</button></>}
  </form>;
}
