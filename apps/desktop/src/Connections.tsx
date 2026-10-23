import { useState } from 'react';
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
