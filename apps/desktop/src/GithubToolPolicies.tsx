import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
type Choice='automatic'|'ask'|'block';
type Setting={conversation_id:string;capability_id:string;policy:Choice;revision:number};
const labels:Record<string,string>={github_file_read:'读取文件',github_file_create:'新建文件',github_file_write:'更新文件'};
export function GithubToolPolicies({conversationId,onChanged}:{conversationId:string;onChanged:()=>Promise<void>}) {
  const [settings,setSettings]=useState<Setting[]>([]);
  const [error,setError]=useState('');const [saving,setSaving]=useState(false);
  const [refresh,setRefresh]=useState(0);const [github,setGithub]=useState('正在读取连接状态…');
  useEffect(()=>{
    let active=true;setSettings([]);setError('');setGithub('正在读取连接状态…');
    void command<Setting[]>('github_tool_policies',{conversationId}).then(rows=>{if(active)setSettings(rows);}).catch(e=>{if(active)setError(errorMessage(e));});
    void command<{configured:boolean;enabled:boolean}>('github_status').then(s=>{if(active)setGithub(s.enabled?'GitHub 总开关已开启':s.configured?'GitHub 总开关已关闭':'GitHub 尚未配置');}).catch(e=>{if(active)setGithub('GitHub 状态不可用：'+errorMessage(e));});
    return ()=>{active=false;};
  },[conversationId,refresh]);
  async function update(setting:Setting,policy:Choice) {
    setSaving(true);setError('');
    try {const next=await command<Setting>('set_github_tool_policy',{conversationId,capabilityId:setting.capability_id,expectedRevision:setting.revision,policy});setSettings(rows=>rows.map(row=>row.capability_id===next.capability_id?next:row));await onChanged().catch(e=>setError('策略已保存，但项目状态刷新失败：'+errorMessage(e)));}
    catch(e){setError(errorMessage(e));}
    finally{setSaving(false);}
  }
  return <details><summary>本对话 GitHub 执行策略</summary>
    <p>{github}</p><p className="muted">默认询问后执行。自动执行允许在 GitHub 总开关及 PAT 可访问仓库范围内连续进行所选操作；拦截会阻止该能力。策略不启用连接、不扩展 PAT 权限，不授予本地目录或终端权限，也不影响其他对话。可在运行期间调整；已派发操作不会被追溯取消。</p>
    {error&&<p className="error">{error}</p>}{!settings.length&&!error&&<p>正在读取策略…</p>}{saving&&<p>正在保存策略…</p>}
    {settings.map(setting=><label key={setting.capability_id}>{labels[setting.capability_id]??setting.capability_id}<select disabled={saving} value={setting.policy} onChange={e=>void update(setting,e.target.value as Choice)}><option value="ask">询问后执行</option><option value="automatic">自动执行</option><option value="block">拦截</option></select><small>策略修订 {setting.revision}</small></label>)}
    <button type="button" disabled={saving} onClick={()=>{setRefresh(n=>n+1);void onChanged().catch(e=>setError(errorMessage(e)));}}>刷新策略和连接状态</button>
  </details>;
}
