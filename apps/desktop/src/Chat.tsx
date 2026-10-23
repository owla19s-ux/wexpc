import type { Conversation, Turn } from './types';

export function Chat({conversation,turns,unsaved,retrySave,busy,draft,setDraft,send,goProject}:{conversation:Conversation;turns:Turn[];unsaved:string[];retrySave:(id:string)=>void;busy:boolean;draft:string;setDraft:(text:string)=>void;send:(prompt:string)=>Promise<void>;goProject:()=>void}) {
  return <section><div className="row"><h1>{conversation.title}</h1>{conversation.project_id&&<button onClick={goProject}>返回项目</button>}</div><p className="muted">{conversation.model_id} · 文本对话 · 工具执行尚未接通</p>
    <div className="messages" aria-live="polite">{!turns.length&&<p>输入需求或讨论项目。当前会话保持独立的模型绑定与历史。</p>}{turns.map(t=><article className="turn" key={t.id}><small>你</small><p className="prewrap">{t.user}</p>{t.assistant&&<><small>AI</small><p className="prewrap">{t.assistant}</p></>}{unsaved.includes(t.id)?<div className="error">请求已结束，结果尚未保存。请保留应用打开。<button onClick={()=>retrySave(t.id)}>重试保存结果</button></div>:t.status==='running'&&<p>正在等待 API 回复…</p>}{t.error&&<p className="error">{t.error}</p>}</article>)}</div>
    <form onSubmit={e=>{e.preventDefault();const text=draft;if(!text.trim()||busy)return;void send(text);}}><label>消息<textarea value={draft} onChange={e=>setDraft(e.target.value)} rows={4}/></label><div className="row"><small>Enter 换行 · 当前请求结束后可发送下一条</small><button className="primary" disabled={busy||!draft.trim()}>{busy?'正在回复':'发送'}</button></div></form>
  </section>;
}
