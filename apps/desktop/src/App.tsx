import { useCallback, useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { command, desktopAvailable, errorMessage } from './client';
import { type Agent, type AgentRole, type Conversation, type Project, type Snapshot } from './types';
import { ConnectionForm, GithubConnection, Selection } from './Connections';
import { ProjectDetail } from './Projects';
import { Chat } from './Chat';
import { Organization } from './Organization';

type Page = 'projects'|'new'|'connections'|'conversation'|'organization';
type ChatDelta = {turnId:string;conversationId:string;delta:string};
export function App() {
  const [data,setData]=useState<Snapshot|null>(null);
  const [error,setError]=useState('');
  const [page,setPage]=useState<Page>('projects');
  const [projectId,setProjectId]=useState('');
  const [conversationId,setConversationId]=useState('');
  const [pending,setPending]=useState<Set<string>>(new Set());
  const [drafts,setDrafts]=useState<Record<string,string>>({});
  const [previews,setPreviews]=useState<Record<string,string>>({});
  const [recovering,setRecovering]=useState<Set<string>>(new Set());
  useEffect(()=>{
    if(!desktopAvailable)return;
    let active=true;
    let unlisten:UnlistenFn|undefined;
    void listen<ChatDelta>('chat-delta',event=>{
      if(!active)return;
      const {turnId,delta}=event.payload;
      if(!turnId||!delta)return;
      setPreviews(current=>({...current,[turnId]:(current[turnId]||'')+delta}));
    }).then(stop=>{if(active)unlisten=stop;else stop();}).catch(e=>setError(errorMessage(e)));
    return ()=>{active=false;unlisten?.();};
  },[]);
  const refresh=useCallback(async()=>{
    const incoming=await command<Snapshot>('snapshot');
    setData(current=>current&&current.revision>incoming.revision?current:incoming);
    return incoming;
  },[]);
  useEffect(()=>{
    if(!data)return;
    const completed=new Set(data.turns.filter(t=>t.assistant!==null||t.status==='succeeded').map(t=>t.id));
    if(!completed.size)return;
    setPreviews(current=>{
      const next={...current};
      let changed=false;
      for(const id of completed){if(id in next){delete next[id];changed=true;}}
      return changed?next:current;
    });
  },[data]);
  useEffect(()=>{if(desktopAvailable) void refresh().catch(e=>setError(errorMessage(e)));},[refresh]);
  async function run(action:()=>Promise<void>) {setError('');try {await action();await refresh();}catch(e){setError(errorMessage(e));}}
  const project=data?.projects.find(p=>p.id===projectId);
  const conversation=data?.conversations.find(c=>c.id===conversationId);
  function showConversation(c:Conversation) {setConversationId(c.id);setPage('conversation');}
  async function createProject(form:HTMLFormElement) {
    const fields=new FormData(form);
    const p=await command<Project>('create_project',{name:fields.get('name'),goal:fields.get('goal'),resources:String(fields.get('resources')||'').split('\n').map(s=>s.trim()).filter(Boolean)});
    setProjectId(p.id);setPage('projects');
  }
  async function send(prompt:string,stream:boolean) {
    if(!conversation) return;
    const id=conversation.id;setError('');setPending(prev=>new Set(prev).add(id));
    // Refresh after request admission; do not await the network before the
    // user can switch to another conversation or project.
    const before=new Set(data?.turns.map(t=>t.id));
    const admitted=(snapshot:Snapshot)=>{
      if(snapshot.turns.some(t=>!before.has(t.id)&&t.conversation_id===id&&t.user===prompt)) {
        setDrafts(prev=>prev[id]===prompt?{...prev,[id]:''}:prev);
      }
    };
    const timer=window.setTimeout(()=>void refresh().then(admitted).catch(()=>{}),200);
    try {await command('send_message',{conversationId:id,prompt,stream});}
    catch(e){setError(errorMessage(e));}
    finally {window.clearTimeout(timer);setPending(prev=>{const next=new Set(prev);next.delete(id);return next;});await refresh().then(admitted).catch(e=>setError(errorMessage(e)));}
  }
  async function resumeReply(turnId:string,mode:"local"|"model") {
    if(!conversation)return;
    const id=conversation.id;
    setError('');setPending(prev=>new Set(prev).add(id));
    setRecovering(prev=>new Set(prev).add(turnId));
    const timer=window.setTimeout(()=>void refresh().catch(()=>{}),200);
    try {await command(mode==='local'?'restore_saved_chat_reply':'resume_tool_reply',{conversationId:id,turnId});}
    catch(error){setError(errorMessage(error));}
    finally {window.clearTimeout(timer);setPending(prev=>{const next=new Set(prev);next.delete(id);return next;});setRecovering(prev=>{const next=new Set(prev);next.delete(turnId);return next;});await refresh().catch(error=>setError(errorMessage(error)));}
  }
  return <div className="app">
    <header><strong>WEXPC</strong><span>0.1.1 · 开发中</span></header>
    <div className="shell"><nav aria-label="主导航">
      <button onClick={()=>setPage('organization')}>Space 与上下文</button>
      <button onClick={()=>{setProjectId('');setPage('projects');}}>项目</button>
      <button onClick={()=>setPage('new')}>＋ 新建项目</button>
      <button onClick={()=>{setConversationId('');setPage('conversation');}}>普通对话</button>
      <button onClick={()=>setPage('connections')}>连接与服务</button>
      {!!data?.projects.length&&<small>最近项目</small>}
      {data?.projects.map(p=><button key={p.id} aria-current={page==='projects'&&p.id===projectId?'page':undefined} onClick={()=>{setProjectId(p.id);setPage('projects');}}>{p.name}</button>)}
      {!!data?.conversations.filter(c=>!c.project_id).length&&<small>独立对话</small>}
      {data?.conversations.filter(c=>!c.project_id).map(c=><button key={c.id} onClick={()=>showConversation(c)}>{c.title}{pending.has(c.id)?' · 回复中':''}</button>)}
    </nav><main>
      {!desktopAvailable&&<p className="notice">浏览器预览未连接桌面服务。请运行桌面程序后管理项目和调用 API。</p>}
      {error&&<p className="error" role="alert">{error}<button onClick={()=>setError('')} aria-label="关闭错误提示">×</button></p>}
      {desktopAvailable&&!data&&!error&&<p>正在加载本地项目…</p>}
      {page==='organization'&&data&&<Organization data={data} run={run}/>}
      {page==='new'&&<section><h1>新建项目</h1><p>记录目标，关联已有代码与资料，再配置参与工作的 AI。</p><form onSubmit={e=>{e.preventDefault();void run(()=>createProject(e.currentTarget));}}>
        <label>项目名称<input name="name" maxLength={128} required /></label>
        <label>目标与约束<textarea name="goal" rows={4} /></label>
        <label>代码与资料地址<textarea id="resources" name="resources" rows={3} placeholder="每行一个本地目录或远程地址" /></label>
        <button type="button" disabled={!desktopAvailable} onClick={()=>void run(async()=>{const dirs=await open({directory:true,multiple:true});if(dirs){const input=document.getElementById('resources') as HTMLTextAreaElement;input.value=[input.value,...(Array.isArray(dirs)?dirs:[dirs])].filter(Boolean).join('\n');}})}>选择本地目录</button>
        <p className="muted">资源关联只用于项目组织，不自动授予 AI 文件或进程权限。</p><button className="primary" disabled={!desktopAvailable}>保存项目</button>
      </form></section>}
      {page==='projects'&&!project&&<section><h1>项目</h1>{data?.projects.length?data.projects.map(p=><button className="item" key={p.id} onClick={()=>setProjectId(p.id)}><strong>{p.name}</strong><span>{p.goal||'尚未填写目标'}</span></button>):<p>从一个真实项目开始，保存目标、资料、AI 分工与任务。</p>}<button className="primary" onClick={()=>setPage('new')}>新建项目</button></section>}
      {page==='projects'&&project&&data&&<ProjectDetail project={project} data={data} run={run} openChat={showConversation} />}
      {page==='connections'&&<section><h1>连接与服务</h1><h2>AI API</h2>{data?.connections.map(c=><div className="item" key={c.id}><strong>{c.name}</strong><span>{c.base_url}</span><span>{c.models.join(' · ')}</span><button onClick={()=>void run(async()=>{await command('set_connection_enabled',{connectionId:c.id,enabled:!c.enabled});})}>{c.enabled?'停用':'启用'}</button></div>)}<ConnectionForm run={run}/><hr/><h2>GitHub</h2><GithubConnection run={run}/><hr/><h2>后续接入项</h2><p>完整本地文件写入、任意终端指令、Git 提交与 Android 模拟器尚未完成桌面执行验收。</p></section>}
      {page==='conversation'&&!conversation&&data&&<section><h1>新建普通对话</h1><form onSubmit={e=>{e.preventDefault();const fields=new FormData(e.currentTarget);void run(async()=>{const c=await command<Conversation>('create_conversation',{projectId:null,title:fields.get('title'),connectionId:fields.get('connection'),modelId:fields.get('model')});showConversation(c);});}}><label>对话名称<input name="title" required maxLength={128} defaultValue="普通对话"/></label><Selection connections={data.connections}/><button className="primary" disabled={!data.connections.some(c=>c.enabled)}>创建对话</button></form></section>}
      {page==='conversation'&&conversation&&data&&<label>关联上下文<select value={conversation.context_id||''} onChange={e=>{const contextId=e.target.value||null;void run(async()=>{await command('bind_conversation_context',{conversationId:conversation.id,contextId});});}}><option value="">独立对话，不关联上下文</option>{data.contexts.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label>}
      {page==='conversation'&&conversation&&data&&<Chat conversation={conversation} turns={data.turns.filter(t=>t.conversation_id===conversation.id)} previews={previews} unsaved={data.unsaved_turn_ids} retrySave={id=>void run(async()=>{await command('retry_chat_result',{turnId:id});})} resume={resumeReply} recovering={recovering} busy={pending.has(conversation.id)} draft={drafts[conversation.id]||''} setDraft={text=>setDrafts(prev=>({...prev,[conversation.id]:text}))} send={send} goProject={()=>{setProjectId(conversation.project_id||'');setPage('projects');}}/>}
    </main></div><footer>{pending.size?`${pending.size} 个对话正在处理`:'本地项目数据由桌面服务保存'} · 任务仍为草案；非流式工具支持人工逐次审批，自动多 Agent 施工尚未完成</footer>
  </div>;
}

export type Run = (action:()=>Promise<void>)=>Promise<void>;
export type OpenChat = (conversation:Conversation)=>void;
export type AgentDraft = {project_id:string;name:string;role:AgentRole;connection_id:string;model_id:string};
export async function createAgent(draft:AgentDraft):Promise<Agent>{return command<Agent>('add_agent',{input:draft});}

