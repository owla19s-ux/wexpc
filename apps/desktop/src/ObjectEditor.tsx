import { useState } from 'react';
import { command } from './client';
import type { Run } from './App';

export function ObjectEditor({kind,record,revision,run}:{kind:'space'|'group'|'context'|'project';record:{id:string;name:string;goal?:string;resources?:string[]};revision:number;run:Run}) {
  const [editing,setEditing]=useState(false);
  const [base,setBase]=useState(revision);
  if(!editing)return <button onClick={()=>{setBase(revision);setEditing(true);}}>编辑{kind==='space'?'空间名称':kind==='group'?'分组名称':'资料'}</button>;
  const rich=kind==='context'||kind==='project';
  return <form onSubmit={e=>{e.preventDefault();const f=new FormData(e.currentTarget);void run(async()=>{
    const args:Record<string,unknown>={revision:base,name:f.get('name')};args[kind+'Id']=record.id;
    if(rich){args.goal=f.get('goal');args.resources=String(f.get('resources')||'').split('\n').map(s=>s.trim()).filter(Boolean);}
    await command(rich?'update_'+kind:'rename_'+kind,args);setEditing(false);
  });}}><label>名称<input name="name" required maxLength={128} defaultValue={record.name}/></label>{rich&&<><label>目标与背景<textarea name="goal" defaultValue={record.goal} rows={3}/></label><label>资料引用<textarea name="resources" defaultValue={record.resources?.join('\n')} rows={3}/></label><small>修改引用不修改外部文件，也不授予访问权限。</small></>}<p className="muted">若保存冲突，请重新读取数据，收起编辑并核对最新资料，再重新编辑。</p><div className="row"><button className="primary">保存修改</button><button type="button" onClick={()=>setEditing(false)}>收起编辑</button><button type="button" onClick={()=>void run(async()=>{})}>重新读取数据</button></div></form>;
}
