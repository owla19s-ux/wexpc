import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
type Access='read'|'write'|'read_write';
type Choice='automatic'|'ask'|'block';
type Directory={id:string;path:string;access:Access;available:boolean};
type Status={revision:number;directories:Directory[]};
type Policy={policy:Choice;revision:number};
function AccessOptions(){return <><option value="read">只读</option><option value="write">只写</option><option value="read_write">读写</option></>;}
export function LocalDirectories({conversationId,onChanged}:{conversationId:string;onChanged:()=>Promise<void>}){
 const [status,setStatus]=useState<Status|null>(null);const [policy,setPolicy]=useState<Policy|null>(null);const [createPolicy,setCreatePolicy]=useState<Policy|null>(null);
 const [access,setAccess]=useState<Access>('read');const [error,setError]=useState('');const [saving,setSaving]=useState(false);const [refresh,setRefresh]=useState(0);
 useEffect(()=>{
  let active=true;setStatus(null);setPolicy(null);setCreatePolicy(null);setError('');
  void command<Status>('local_directory_status',{conversationId}).then(s=>{if(active)setStatus(s);}).catch(e=>{if(active)setError(errorMessage(e));});
  void command<Policy>('local_read_policy',{conversationId}).then(s=>{if(active)setPolicy(s);}).catch(e=>{if(active)setError(errorMessage(e));});
  void command<Policy>('local_create_policy',{conversationId}).then(s=>{if(active)setCreatePolicy(s);}).catch(e=>{if(active)setError(errorMessage(e));});
  return ()=>{active=false;};
 },[conversationId,refresh]);
 async function mutate(name:string,args:Record<string,unknown>){
  if(!status)return;setSaving(true);setError('');
  try{setStatus(await command<Status>(name,{conversationId,expectedRevision:status.revision,...args}));}
  catch(e){setError(errorMessage(e));}finally{setSaving(false);}
 }
 async function updatePolicy(choice:Choice){
  if(!policy)return;setSaving(true);setError('');
  try{setPolicy(await command<Policy>('set_local_read_policy',{conversationId,expectedRevision:policy.revision,policy:choice}));await onChanged().catch(e=>setError('策略已保存，但项目状态刷新失败：'+errorMessage(e)));}
  catch(e){setError(errorMessage(e));}finally{setSaving(false);}
 }
 async function updateCreatePolicy(choice:Choice){
  if(!createPolicy)return;setSaving(true);setError('');
  try{setCreatePolicy(await command<Policy>('set_local_create_policy',{conversationId,expectedRevision:createPolicy.revision,policy:choice}));await onChanged().catch(e=>setError('新建策略已保存，但项目状态刷新失败：'+errorMessage(e)));}
  catch(e){setError(errorMessage(e));}finally{setSaving(false);}
 }
 return <details><summary>本对话本地目录与读写策略</summary>
  <p className="muted">选择目录后，授权保存至撤销，并在重启后核对原目录身份。可添加多个目录、分别调整读写权限；项目或 Space 关联不会产生授权，也不会授权其他对话。选择目录包含对该目录的明确访问同意。</p>
  <p className="muted">当前可读取授权目录内的直接文件。已提供授权目录内的独占新建文本文件（不覆盖既有文件），普通覆盖修改、子目录文件操作及终端持续授权尚不可用。当前直接文件文本上限 64 KiB。</p>
  {error&&<p className="error" role="alert">{error}</p>}{saving&&<p>正在保存或等待目录选择…</p>}
  {policy&&<label>本地文件读取策略<select aria-label="本地文件读取策略" value={policy.policy} disabled={saving} onChange={e=>void updatePolicy(e.target.value as Choice)}><option value="ask">询问后执行</option><option value="automatic">自动执行</option><option value="block">拦截</option></select><small>自动执行仍需当前对话的目录读取授权；拦截不会删除已保存的目录范围。修订 {policy.revision}</small></label>}
  {createPolicy&&<label>本地文件新建策略<select aria-label="本地文件新建策略" value={createPolicy.policy} disabled={saving} onChange={e=>void updateCreatePolicy(e.target.value as Choice)}><option value="ask">询问后执行</option><option value="automatic">自动执行</option><option value="block">拦截</option></select><small>仅允许已授权写权限目录内独占新建文件，绝不覆盖现有文件。修订 {createPolicy.revision}</small></label>}
  <label>新目录权限<select value={access} disabled={saving} onChange={e=>setAccess(e.target.value as Access)}><AccessOptions/></select></label>
  <button type="button" disabled={saving||!status} onClick={()=>void mutate('authorize_local_directory',{access})}>选择并授权目录</button>
  <button type="button" disabled={saving} onClick={()=>setRefresh(n=>n+1)}>刷新目录和读取策略</button>
  {!status&&!error&&<p>正在读取目录授权…</p>}{status&&!status.directories.length&&<p>尚未授权目录。</p>}
  {status?.directories.map(d=><article key={d.id}><p>{d.path}</p>{!d.available&&<p className="error">目录身份无法核对，请核查位置或撤销后重新选择。</p>}<label>目录权限 {d.path}<select value={d.access} disabled={saving} onChange={e=>void mutate('edit_local_directory',{directoryId:d.id,access:e.target.value})}><AccessOptions/></select></label><button type="button" disabled={saving} aria-label={'撤销 '+d.path} onClick={()=>void mutate('revoke_local_directory',{directoryId:d.id})}>撤销授权</button></article>)}
  {status&&<small>授权修订 {status.revision} · 已派发的读取不会追溯取消；撤销后新读取将被拒绝。</small>}
 </details>;
}
