import {SavedToolResults} from './SavedToolResults';
import { useEffect, useState } from 'react';
import { command, errorMessage } from './client';
type Receipt = { operation_id:string; attempt_id:string; state:string; turn_id:string|null; content_digest:string|null };
const labels:Record<string,string>={in_flight:'待核查：已记录派发准备，尚无终态',succeeded:'执行成功（未独立核验）',failed:'执行失败',unknown:'结果未知，须先核查'};
export function ToolReceipts({conversationId,busy}:{conversationId:string;busy:boolean}) {
  const [selectedTurn,setSelectedTurn]=useState<string|null>(null);
  const [items,setItems]=useState<Receipt[]>([]);
  const [error,setError]=useState('');
  const [refresh,setRefresh]=useState(0);
  const [loading,setLoading]=useState(true);
  useEffect(()=>{
    let active=true;setItems([]);setError('');setLoading(true);
    command<Receipt[]>('tool_receipts',{conversationId}).then(rows=>{if(active)setItems(rows);}).catch(e=>{if(active)setError(errorMessage(e));}).finally(()=>{if(active)setLoading(false);});
    return ()=>{active=false;};
  },[conversationId,busy,refresh]);
  return <details><summary>工具执行记录（{items.length}）</summary>
    <p className="muted">展示本对话的执行状态和摘要，可按回合查看已保存的工具输出。记录不代表授权、独立验证或可自动重试；模型回复失败也不代表工具未执行。旧记录缺少对话关联时不会归入本对话。</p>
    <button type="button" onClick={()=>setRefresh(n=>n+1)}>刷新执行记录</button>
    {error&&<p className="error">{error}</p>}
    {loading&&<p>正在读取执行记录…</p>}
    {!loading&&!error&&!items.length&&<p>暂无可关联的执行记录。</p>}
    {[...new Set(items.map(item=>item.turn_id).filter((id):id is string=>id!==null))].map(turn=><button type="button" key={turn} onClick={()=>setSelectedTurn(turn)}>查看回合输出 · {turn}</button>)}
    {selectedTurn&&<><button type="button" onClick={()=>setSelectedTurn(null)}>收起输出</button><SavedToolResults key={conversationId+selectedTurn} conversationId={conversationId} turnId={selectedTurn}/></>}
    {items.map(item=><article key={item.attempt_id}><p>{labels[item.state]??'状态待核查'}</p><small>回合：{item.turn_id??'未关联'} · 操作：{item.operation_id} · 尝试：{item.attempt_id}</small>{item.content_digest&&<p className="muted">摘要：{item.content_digest}</p>}</article>)}
  </details>;
}
