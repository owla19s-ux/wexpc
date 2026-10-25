import {useEffect,useRef,useState} from 'react';
import {command,errorMessage} from './client';
type Process={conversation_id:string;attempt_id:string;operation_id:string;program:string;arguments:string[];elapsed_ms:number;cancellation_requested:boolean;finished?:boolean};
export function ActiveProcesses({conversationId,busy}:{conversationId:string;busy:boolean}) {
  const current=useRef(conversationId);current.current=conversationId;
  const [snapshot,setSnapshot]=useState<{conversationId:string;rows:Process[]}>({conversationId,rows:[]});
  const [failure,setFailure]=useState<{conversationId:string;text:string}>({conversationId,text:''});
  const [cancelFailure,setCancelFailure]=useState<{conversationId:string;text:string}>({conversationId,text:''});
  const [sending,setSending]=useState<Set<string>>(new Set());
  const [querying,setQuerying]=useState(false);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    let active=true;let reading=false;
    async function read() {
      if(reading)return;reading=true;if(active)setQuerying(true);
      try {
        const rows=await command<Process[]>('active_processes',{conversationId});
        if(active){setSnapshot({conversationId,rows:rows.filter(p=>p.conversation_id===conversationId)});setFailure({conversationId,text:''});}
      }catch(e){if(active)setFailure({conversationId,text:errorMessage(e)});}
      finally{reading=false;if(active)setQuerying(false);}
    }
    void read();const timer=window.setInterval(()=>void read(),1000);
    return ()=>{active=false;window.clearInterval(timer);};
  },[conversationId,busy,refresh]);
  const items=snapshot.conversationId===conversationId?snapshot.rows:[];
  const stale= failure.conversationId===conversationId && Boolean(failure.text);
  async function cancel(attemptId:string) {
    const id=conversationId;const key=id+':'+attemptId;
    setSending(old=>new Set(old).add(key));setCancelFailure({conversationId:id,text:''});
    try {await command('cancel_process',{conversationId:id,attemptId});if(current.current===id)setRefresh(n=>n+1);}
    catch(e){if(current.current===id)setCancelFailure({conversationId:id,text:errorMessage(e)});}
    finally{setSending(old=>{const next=new Set(old);next.delete(key);return next;});}
  }
  return <section aria-label="本地进程控制">
    <h3>当前本地进程</h3>
    <button type="button" onClick={()=>setRefresh(n=>n+1)}>重新查询进程</button>
    {stale&&!!items.length&&<p className="muted">以下是上次成功查询的进程，当前状态尚未核实；重新查询成功前不能取消。</p>}
    {failure.conversationId===conversationId&&failure.text&&<p className="error">{failure.text}</p>}
    {cancelFailure.conversationId===conversationId&&cancelFailure.text&&<p className="error">{cancelFailure.text}</p>}
    {!items.length&&<p className="muted">当前没有查询到本会话的运行进程；历史结果请查看工具执行记录。</p>}
    {items.map(item=><article key={item.attempt_id}>
      <p>{item.program} {JSON.stringify(item.arguments)}</p><small>尝试已登记 {Math.floor(item.elapsed_ms/1000)} 秒 · 尝试 {item.attempt_id}</small>
      {item.finished?<p>执行器已取得终态，正在保存回执。</p>:item.cancellation_requested?<p>已请求取消，等待执行器回收并保存回执。</p>:<button type="button" disabled={stale||querying||sending.has(conversationId+':'+item.attempt_id)} onClick={()=>void cancel(item.attempt_id)}>取消此进程</button>}
    </article>)}
    {!!items.length&&<p className="muted">取消不会回滚已经发生的副作用；结果需按保存回执核查，不会自动重跑。</p>}
  </section>;
}
