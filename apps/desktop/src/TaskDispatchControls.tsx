import { useEffect, useRef, useState } from 'react';
import { command } from './client';
import type { Run } from './App';
import type { Agent, DispatchAttempt, Task } from './types';
import { roleNames } from './types';

type Preview = {taskId:string;agentId:string;revision:number;publicationEpoch:number;assignmentEpoch:number};

export function TaskDispatchControls({task,agents,revision,attempts,run}:{
  task:Task;agents:Agent[];revision:number;attempts:DispatchAttempt[];run:Run;
}) {
  const [agentId,setAgentId]=useState('');
  const [preview,setPreview]=useState<Preview|null>(null);
  const [queueing,setQueueing]=useState(false);
  const generation=useRef(0);
  const epoch=task.publication_epoch;
  const selected=Boolean(agentId&&task.agent_ids.includes(agentId));
  const taskAttempts=attempts.filter(a=>a.task_id===task.id);
  const alreadyQueued=taskAttempts.some(a=>a.publication_epoch===epoch);
  const statusText:Record<DispatchAttempt['status'],string>={
    pending:'待领取（尚未执行）',
    in_flight:'已领取（不代表执行完成）',
    outcome_unknown:'结果未知（须核查，禁止自动重试）',
  };
  const currentPreview=preview&&preview.taskId===task.id&&preview.agentId===agentId
    &&preview.revision===revision&&preview.publicationEpoch===epoch?preview:null;

  useEffect(()=>{
    generation.current++;
    setPreview(null);
  },[task.id,revision,epoch]);
  useEffect(()=>{setAgentId('');},[task.id,epoch]);

  const previewAdmission=()=>{
    if(!selected)return;
    const request=++generation.current;
    setPreview(null);
    const chosen=agentId;
    const taskId=task.id;
    const snapshotRevision=revision;
    const publicationEpoch=epoch;
    void run(async()=>{
      const ticket=await command<{taskId:string;projectId:string;agentId:string;assignmentEpoch:number}>(
        'preview_task_dispatch',{revision:snapshotRevision,taskId,agentId:chosen});
      if(request===generation.current&&ticket.taskId===taskId&&ticket.agentId===chosen){
        setPreview({taskId,agentId:chosen,revision:snapshotRevision,publicationEpoch:publicationEpoch??0,assignmentEpoch:ticket.assignmentEpoch});
      }
    });
  };
  const queue=()=>{
    if(!selected||alreadyQueued||queueing)return;
    generation.current++;
    setPreview(null);
    setQueueing(true);
    void run(async()=>{
      try {
        await command('queue_dispatch_attempt',{revision,taskId:task.id,agentId});
      } finally {
        setQueueing(false);
      }
    });
  };

  return <div>
    <p>任务已发布；编辑请通过后续任务修订流程，尚未实际派发或执行。</p>
    <label>选择派工成员
      <select value={selected?agentId:''} onChange={e=>{
        generation.current++;
        setPreview(null);
        setAgentId(e.target.value);
      }}>
        <option value="">请选择已分配成员</option>
        {task.agent_ids.map(id=>{
          const agent=agents.find(a=>a.id===id);
          return <option key={id} value={id}>{agent?agent.name+' · '+roleNames[agent.role]:`未知成员（${id}）`}</option>;
        })}
      </select>
    </label>
    <button type="button" disabled={!selected||queueing} onClick={previewAdmission}>核对调度准入（不执行）</button>
    {currentPreview&&<p role="status">仅通过准入预检：{currentPreview.agentId}（分配版本 {currentPreview.assignmentEpoch}，读取修订 {currentPreview.revision}），未占用资源或启动工具。</p>}
    <button type="button" disabled={!selected||alreadyQueued||queueing} onClick={queue}>登记待派工尝试（不启动工具）</button>
    {alreadyQueued&&<p className="muted">当前发布版本已登记派工尝试，不能重复登记；如结果未知，必须先核查实际执行事实。</p>}
    {taskAttempts.map(a=>{
      const assignee=agents.find(agent=>agent.id===a.agent_id);
      return <p key={a.id}>派工记录 {a.id} · 成员 {assignee?.name||`未知成员（${a.agent_id}）`}（{a.agent_id}） · {statusText[a.status]} · 发布版本 {a.publication_epoch}</p>;
    })}
  </div>;
}
