import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
import {ProcessOutput} from './ProcessOutput';
type SavedResult={record_id:string;call_id:string;tool_name:string;result:string;arguments_json?:string;sequence?:number|null};
export function SavedToolResults({conversationId,turnId,busy}:{conversationId:string;turnId:string;busy:boolean}) {
  const [snapshot,setSnapshot]=useState<{conversationId:string;turnId:string;rows:SavedResult[]}>({conversationId,turnId,rows:[]});
  const [error,setError]=useState<{conversationId:string;turnId:string;text:string}>({conversationId,turnId,text:''});
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    // This read-only query is safe during an in-flight request. Always load on
    // first expansion, then refresh when busy changes without hiding old evidence.
    let active=true;setLoading(true);setError({conversationId,turnId,text:''});
    command<SavedResult[]>('tool_results',{conversationId,turnId}).then(rows=>{if(active)setSnapshot({conversationId,turnId,rows});}).catch(e=>{if(active)setError({conversationId,turnId,text:errorMessage(e)});}).finally(()=>{if(active)setLoading(false);});
    return ()=>{active=false;};
  },[conversationId,turnId,busy,refresh]);
  const items=snapshot.conversationId===conversationId&&snapshot.turnId===turnId?snapshot.rows:[];
  const visibleError=error.conversationId===conversationId&&error.turnId===turnId?error.text:'';
  return <section><p>回合 {turnId} 的已保存工具输出。仅供核查，不会重新执行或发送给模型。</p>
    <button type="button" onClick={()=>setRefresh(n=>n+1)}>刷新输出</button>
    {busy&&<p className="muted">请求仍在执行，已保存输出可能继续增加；请求结束后会自动刷新。</p>}
    {loading&&<p>{items.length?'正在更新已保存输出…':'正在读取…'}</p>}
    {visibleError&&<p className="error">{items.length?'刷新失败，以下仍为上次读取的输出：':''}{visibleError}</p>}
    {!loading&&!visibleError&&!items.length&&<p>没有已保存的正文；旧记录可能只有状态摘要，不能推断工具未执行。</p>}
    {items.map(item=><details key={item.record_id}><summary>{item.sequence==null?'旧记录：顺序未记录':`保存顺序 ${item.sequence}`} · {item.tool_name} · {item.call_id}</summary>{['run_command','cargo_test','cargo_fmt_check'].includes(item.tool_name)?<ProcessOutput body={item.result} argumentsJson={item.arguments_json}/>:<pre className="prewrap">{item.result}</pre>}</details>)}
  </section>;
}
