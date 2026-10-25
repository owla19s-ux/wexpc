import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
type SavedResult={record_id:string;call_id:string;tool_name:string;result:string};
export function SavedToolResults({conversationId,turnId}:{conversationId:string;turnId:string}) {
  const [items,setItems]=useState<SavedResult[]>([]);
  const [error,setError]=useState('');
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    let active=true;setLoading(true);setItems([]);setError('');
    command<SavedResult[]>('tool_results',{conversationId,turnId}).then(rows=>{if(active)setItems(rows);}).catch(e=>{if(active)setError(errorMessage(e));}).finally(()=>{if(active)setLoading(false);});
    return ()=>{active=false;};
  },[conversationId,turnId,refresh]);
  return <section><p>回合 {turnId} 的已保存工具输出。仅供核查，不会重新执行或发送给模型。</p>
    <button type="button" onClick={()=>setRefresh(n=>n+1)}>刷新输出</button>
    {loading&&<p>正在读取…</p>}{error&&<p className="error">{error}</p>}
    {!loading&&!error&&!items.length&&<p>没有已保存的正文；旧记录可能只有状态摘要，不能推断工具未执行。</p>}
    {items.map(item=><details key={item.record_id}><summary>{item.tool_name} · {item.call_id}</summary><pre className="prewrap">{item.result}</pre></details>)}
  </section>;
}
