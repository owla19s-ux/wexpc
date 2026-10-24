import { useEffect, useState } from 'react';
import { command } from './client';
import type { AiConnection } from './types';
import type { Run } from './App';

export function Selection({connections}:{connections:AiConnection[]}) {
  const available=connections.filter(c=>c.enabled);
  const [selected,setSelected]=useState('');
  const active=available.find(c=>c.id===selected)||available[0];
  return <div className="fields"><label>API 连接<select name="connection" required value={active?.id||''} onChange={e=>setSelected(e.target.value)}><option value="" disabled>请先配置连接</option>{available.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label><label>模型<select key={active?.id} name="model" required><option value="" disabled>选择模型</option>{active?.models.map(m=><option key={m} value={m}>{m}</option>)}</select></label></div>;
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
  const refresh=()=>command<GithubStatus>('github_status').then(setStatus).catch(e=>setFailure(String(e)));
  useEffect(()=>{void refresh();},[]);
  const save=(form:HTMLFormElement)=>{
    const token=String(new FormData(form).get('token')||'');
    const input=form.elements.namedItem('token') as HTMLInputElement;
    input.value='';
    void run(async()=>{await command('configure_github',{token});await refresh();});
  };
  return <div>
    <p>{status?.configured?(status.enabled?'GitHub 已启用（操作仍逐次审批）':'GitHub 已配置，尚未启用'):'尚未配置 GitHub 凭证'}</p>
    {failure&&<p className="error">{failure}</p>}
    <form onSubmit={e=>{e.preventDefault();save(e.currentTarget);}}>
      <label>GitHub Personal Access Token<input name="token" type="password" autoComplete="off" required/></label>
      <p className="muted">凭证仅保存到 Windows 凭证管理器。更换凭证会关闭全局开关；不会从项目关联推断授权。</p>
      <button type="submit">保存 GitHub 凭证</button>
    </form>
    {status?.configured&&<button type="button" onClick={()=>void run(async()=>{
      await command('set_github_enabled',{enabled:!status.enabled});
      await refresh();
    })}>{status.enabled?'关闭 GitHub 全局访问':'启用 GitHub 全局访问'}</button>}
    <p className="muted">总开关只允许 AI 提出 GitHub 操作。具体仓库、文件及写入内容仍需要逐次确认。</p>
  </div>;
}
