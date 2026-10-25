import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
import {ProcessOutput} from './ProcessOutput';
type SavedResult={record_id:string;call_id:string;tool_name:string;result:string;sequence?:number|null};
export function SavedToolResults({conversationId,turnId,busy}:{conversationId:string;turnId:string;busy:boolean}) {
  const [items,setItems]=useState<SavedResult[]>([]);
  const [error,setError]=useState('');
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    // This read-only query is safe during an in-flight request. Always load on
    // first expansion, then refresh when busy changes without hiding old evidence.
    let active=true;setLoading(true);setError('');
    command<SavedResult[]>('tool_results',{conversationId,turnId}).then(rows=>{if(active)setItems(rows);}).catch(e=>{if(active)setError(errorMessage(e));}).finally(()=>{if(active)setLoading(false);});
    return ()=>{active=false;};
  },[conversationId,turnId,busy,refresh]);
  return <section><p>回合 {turnId} 的已保存工具输出。仅供核查，不会重新执行或发送给模型。</p>
    <button type="button" onClick={()=>setRefresh(n=>n+1)}>刷新输出</button>
    {busy&&<p className="muted">请求仍在执行，已保存输出可能继续增加；请求结束后会自动刷新。</p>}
    {loading&&<p>{items.length?'正在更新已保存输出…':'正在读取…'}</p>}
    {error&&<p className="error">{items.length?'刷新失败，以下仍为上次读取的输出：':''}{error}</p>}
    {!loading&&!error&&!items.length&&<p>没有已保存的正文；旧记录可能只有状态摘要，不能推断工具未执行。</p>}
    {items.map(item=><details key={item.record_id}><summary>{item.sequence==null?'旧记录：顺序未记录':`保存顺序 ${item.sequence}`} · {item.tool_name} · {item.call_id}</summary>{item.tool_name==='run_command'?<ProcessOutput body={item.result}/>:<pre className="prewrap">{item.result}</pre>}</details>)}
  </section>;
}
