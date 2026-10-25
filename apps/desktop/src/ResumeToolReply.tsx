import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
type Availability={available:boolean;mode:"local"|"model"|"none";reason:string};
export function ResumeToolReply({conversationId,turnId,busy,resume}:{conversationId:string;turnId:string;busy:boolean;resume:(id:string,mode:"local"|"model",continueTools?:boolean)=>Promise<void>}) {
  const [state,setState]=useState<Availability|null>(null);
  const [refresh,setRefresh]=useState(0);
  const [continueTools,setContinueTools]=useState(false);
  useEffect(()=>{
    let active=true;setState(null);
    if(!busy)void command<Availability>('tool_reply_recovery',{conversationId,turnId}).then(value=>{if(active)setState(value);}).catch(error=>{if(active)setState({available:false,mode:"none",reason:errorMessage(error)});});
    return ()=>{active=false;};
  },[conversationId,turnId,busy,refresh]);
  return <div><p className="muted">{state?.reason??(busy?'当前对话正在处理请求。':'正在检查可恢复的回复…')}</p>
    {state?.available&&state.mode==='model'&&<label className="check"><input type="checkbox" disabled={busy} checked={continueTools} onChange={e=>setContinueTools(e.target.checked)}/>恢复后继续工具流程（只处理模型的新请求，仍按当前审批）</label>}
    <button type="button" disabled={busy||!state?.available} onClick={()=>{if(state?.mode==='local'||state?.mode==='model')void resume(turnId,state.mode,state.mode==='model'&&continueTools);}}>{state?.mode==='local'?'恢复已取得的回复（不请求 API）':continueTools?'发送回执，继续工具流程':'发送已保存回执，恢复回复'}</button>
    <button type="button" disabled={busy} onClick={()=>setRefresh(n=>n+1)}>重新检查恢复条件</button>
  </div>;
}
