import { useState } from 'react';
import type { Conversation, Turn } from './types';

export function Chat({conversation,turns,previews,unsaved,retrySave,busy,draft,setDraft,send,goProject}:{conversation:Conversation;turns:Turn[];previews:Record<string,string>;unsaved:string[];retrySave:(id:string)=>void;busy:boolean;draft:string;setDraft:(text:string)=>void;send:(prompt:string,stream:boolean)=>Promise<void>;goProject:()=>void}) {
  const [stream,setStream]=useState(false);
  return <section><div className="row"><h1>{conversation.title}</h1>{conversation.project_id&&<button onClick={goProject}>返回项目</button>}</div><p className="muted">{conversation.model_id} · 文本对话 · 非流式支持逐次审批的文件读取、Git 状态/差异、Cargo 检查和 GitHub 文件读取/新建/更新</p>
    <div className="messages" aria-live="polite">{!turns.length&&<p>输入需求或讨论项目。当前会话保持独立的模型绑定与历史。</p>}{turns.map(t=><article className="turn" key={t.id}><small>你</small><p className="prewrap">{t.user}</p>{t.assistant&&<><small>AI</small><p className="prewrap">{t.assistant}</p></>}{!t.assistant&&previews[t.id]&&<><small>AI · 临时预览{t.status==='failed'?'（请求未完成）':''}</small><p className="prewrap">{previews[t.id]}</p></>}{unsaved.includes(t.id)?<div className="error">请求已结束，结果尚未保存。请保留应用打开。<button onClick={()=>retrySave(t.id)}>重试保存结果</button></div>:t.status==='running'&&<p>{previews[t.id]?'正在继续接收…':'正在等待 API 回复…'}</p>}{t.error&&<p className="error">{t.error}</p>}</article>)}</div>
    <form onSubmit={e=>{e.preventDefault();const text=draft;if(!text.trim()||busy)return;void send(text,stream);}}><label>消息<textarea value={draft} onChange={e=>setDraft(e.target.value)} rows={4}/></label><div className="row"><label className="check"><input type="checkbox" checked={stream} onChange={e=>setStream(e.target.checked)}/>流式预览（暂不使用工具）</label><small>Enter 换行 · 需工具操作请关闭流式预览 · 工具调用均须逐次审批</small><button className="primary" disabled={busy||!draft.trim()}>{busy?'正在回复':'发送'}</button></div></form>
  </section>;
}
