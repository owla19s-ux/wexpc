import { useEffect, useRef, useState } from 'react';
import { command, errorMessage } from './client';
import type { AiConnection } from './types';
import type { Run } from './App';

export function Selection({connections}:{connections:AiConnection[]}) {
  const available=connections.filter(c=>c.enabled);
  const [selected,setSelected]=useState('');
  const active=available.find(c=>c.id===selected)||available[0];
  return <div className="fields"><label>API 连接<select name="connection" required value={active?.id||''} onChange={e=>setSelected(e.target.value)}><option value="" disabled>请先配置连接</option>{available.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label><label>模型<select key={active?.id} name="model" required><option value="" disabled>选择模型</option>{active?.models.map(m=><option key={m} value={m}>{m}</option>)}</select></label></div>;
}

// Both ordinary conversations and project Agent creation require a verified
// Host connection snapshot. Cached enabled flags remain visible for reference,
// but cannot be used to submit a new connection-bound object.
export function VerifiedConnectionCreation({connections, snapshotFresh, label}:{
  connections: AiConnection[];
  snapshotFresh: boolean;
  label: string;
}) {
  const available=snapshotFresh && connections.some(connection=>connection.enabled);
  return <>
    <Selection connections={snapshotFresh ? connections : []}/>
    {!snapshotFresh && <p className="notice" role="status">API 连接状态尚未核实：请重新读取桌面服务快照后再创建。</p>}
    <button type="submit" className="primary" disabled={!available}>{label}</button>
    {snapshotFresh && !available && <p>请先在连接与服务中配置并启用 API。</p>}
  </>;
}

// A cached Snapshot may no longer reflect the Host's enabled state. Never
// invert a stale value; the next verified snapshot is required before toggling.
// The Host remains the authority for the actual connection state.
export function ApiConnectionControls({connections, snapshotFresh, run}:{
  connections: AiConnection[];
  snapshotFresh: boolean;
  run: Run;
}) {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  async function toggle(connection: AiConnection) {
    setPendingIds(current => new Set(current).add(connection.id));
    try {
      await run(async () => {
        await command('set_connection_enabled', {connectionId: connection.id, enabled: !connection.enabled});
      });
    } finally {
      setPendingIds(current => {
        const next = new Set(current);
        next.delete(connection.id);
        return next;
      });
    }
  }
  return <>
    {!snapshotFresh && <p className="notice" role="status">API 连接启停已暂停：当前仅有未核实的快照。请先重新读取桌面服务状态。</p>}
    {connections.map(connection => <div className="item" key={connection.id}>
      <strong>{connection.name}</strong>
      <span>{connection.base_url}</span>
      <span>{connection.models.join(' · ')}</span>
      <button type="button" disabled={!snapshotFresh || pendingIds.has(connection.id)}
        onClick={() => void toggle(connection)}>{connection.enabled ? '停用' : '启用'}</button>
    </div>)}
  </>;
}

export function ConnectionForm({run}:{run:Run}) {
  return <details><summary>添加 API 连接</summary><form onSubmit={e=>{e.preventDefault();const form=e.currentTarget;const values=new FormData(form);const key=String(values.get('key')||'');(form.elements.namedItem('key') as HTMLInputElement).value='';void run(async()=>{await command('add_connection',{input:{name:values.get('name'),base_url:values.get('url'),models:String(values.get('models')||'').split('\n').map(s=>s.trim()).filter(Boolean)},apiKey:key});form.reset();});}}>
    <label>连接名称<input name="name" required maxLength={128} placeholder="例如 DeepSeek 官方" /></label>
    <label>API Base URL<input name="url" required type="url" defaultValue="https://api.deepseek.com" /></label>
    <label>模型名称<textarea name="models" required rows={2} defaultValue="deepseek-chat" /></label><small>每行一个模型；同一连接可以配置多个模型。</small>
    <label>API Key<input name="key" required type="password" autoComplete="off" /></label>
    <p className="muted">密钥由 Windows 凭证管理器保存，界面不回读密钥。</p>
    <button className="primary">保存连接</button>
  </form></details>;
}

type GithubStatus = {configured:boolean;enabled:boolean};
export function GithubConnection({run}:{run:Run}) {
  const [status,setStatus]=useState<GithubStatus|null>(null);
  const [failure,setFailure]=useState('');
  const [checking,setChecking]=useState(true);
  const querySequence=useRef(0);
  const refresh=async()=>{
    const sequence=++querySequence.current;
    setChecking(true);
    try {
      const next=await command<GithubStatus>('github_status');
      if(sequence===querySequence.current){setStatus(next);setFailure('');}
    }catch(e){
      if(sequence===querySequence.current)setFailure(errorMessage(e));
      throw e;
    }finally{
      if(sequence===querySequence.current)setChecking(false);
    }
  };
  useEffect(()=>{
    void refresh().catch(()=>{});
    return ()=>{querySequence.current++;};
  },[]);
  const verified=status!==null&&!checking&&!failure;
  const description=status?.configured
    ? status.enabled?'GitHub 已启用（按各对话执行策略）':'GitHub 已配置，尚未启用'
    : '尚未配置 GitHub 凭证';
  const save=(form:HTMLFormElement)=>{
    const token=String(new FormData(form).get('token')||'');
    const input=form.elements.namedItem('token') as HTMLInputElement;
    input.value='';
    void run(async()=>{await command('configure_github',{token});await refresh();});
  };
  return <div>
    <p>{verified?description:status?`上次核对：${description}；当前状态尚未核实。`:'GitHub 状态尚未核实，不能判断是否配置凭证。'}</p>
    {checking&&<p className="muted">正在核对 GitHub 连接状态…</p>}
    {failure&&<p className="error" role="alert">GitHub 状态查询失败：{failure}。请重新查询后再调整访问开关或撤销凭证。</p>}
    <button type="button" disabled={checking} onClick={()=>void refresh().catch(()=>{})}>重新查询 GitHub 状态</button>
    <form onSubmit={e=>{e.preventDefault();save(e.currentTarget);}}>
      <label>GitHub Personal Access Token<input name="token" type="password" autoComplete="off" required/></label>
      <p className="muted">凭证仅保存到 Windows 凭证管理器。更换凭证会关闭全局开关；不会从项目关联推断授权。</p>
      <button type="submit">保存 GitHub 凭证</button>
    </form>
    {status?.configured&&<button type="button" disabled={!verified} onClick={()=>void run(async()=>{
      await command('set_github_enabled',{enabled:!status.enabled});
      await refresh();
    })}>{status.enabled?'关闭 GitHub 全局访问':'启用 GitHub 全局访问'}</button>}
    {status?.configured&&<button type="button" disabled={!verified} onClick={()=>{
      if(!window.confirm('删除本机 GitHub 凭证并关闭 WEXPC 后续访问？已派发操作不会自动撤销；此操作不会使 GitHub 服务端的 Token 失效。'))return;
      void run(async()=>{await command('revoke_github');await refresh();});
    }}>删除本机 GitHub 凭证并关闭访问</button>}
    <p className="muted">开发测试阶段，总开关允许在 PAT 已授权的所有仓库范围内读写。各对话分别选择自动执行、询问后执行或拦截；总开关不扩展 PAT 权限。</p>
  </div>;
}
