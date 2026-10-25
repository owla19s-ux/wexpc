import {useEffect,useState} from 'react';
import {command,errorMessage} from './client';
type Availability={available:boolean;reason:string};
export function ResumeToolReply({conversationId,turnId,busy,resume}:{conversationId:string;turnId:string;busy:boolean;resume:(id:string)=>Promise<void>}) {
  const [state,setState]=useState<Availability|null>(null);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    let active=true;setState(null);
    if(!busy)void command<Availability>('tool_reply_recovery',{conversationId,turnId}).then(value=>{if(active)setState(value);}).catch(error=>{if(active)setState({available:false,reason:errorMessage(error)});});
    return ()=>{active=false;};
  },[conversationId,turnId,busy,refresh]);
  return <div><p className="muted">{state?.reason??(busy?'当前对话正在处理请求。':'正在检查可恢复的工具回合…')}</p>
    <button type="button" disabled={busy||!state?.available} onClick={()=>void resume(turnId)}>发送已保存回执，恢复回复</button>
    <button type="button" disabled={busy} onClick={()=>setRefresh(n=>n+1)}>重新检查恢复条件</button>
  </div>;
}
