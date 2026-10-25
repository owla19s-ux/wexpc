import { useEffect, useState } from 'react';
import { command, errorMessage } from './client';
import { roleNames, type MemberWorkBrief } from './types';

export function MemberBriefPanel({agentId,revision,close}:{agentId:string;revision:number;close:()=>void}) {
  const [brief,setBrief]=useState<MemberWorkBrief|null>(null);
  const [error,setError]=useState('');
  const [attempt,setAttempt]=useState(0);
  useEffect(()=>{
    let active=true;
    setBrief(null);setError('');
    void command<MemberWorkBrief>('member_brief',{agentId}).then(value=>{
      if(active)setBrief(value);
    }).catch(reason=>{if(active)setError(errorMessage(reason));});
    return ()=>{active=false;};
  },[agentId,revision,attempt]);
  return <aside className="task-detail" aria-label="成员工作资料">
    <div className="row"><h3>成员工作资料</h3><button onClick={close}>收起资料</button></div>
    <p className="muted">可先核对该成员将参考的项目资料。实际发送时会重新读取最新记录；查看资料不会调用模型。该成员自己的聊天记录和显式关联的对话上下文会另行参考。</p>
    {error&&<p className="error" role="alert">{error}<button onClick={()=>setAttempt(v=>v+1)}>重新读取</button></p>}
    {!brief&&!error&&<p>正在读取工作资料…</p>}
    {brief&&<>
      <p><strong>{brief.member.name} · {roleNames[brief.member.role]}</strong> · {brief.member.model_id}</p>
      <p>{brief.scope==='project'?'主 AI 参考本项目全部任务与成员，包含未分配任务。':'该成员参考当前分配给自己的任务。'}</p>
      <h4>{brief.project.name}</h4><p className="prewrap">{brief.project.goal||'尚未填写项目目标'}</p>
      {!!brief.project.resources.length&&<details><summary>项目资料引用</summary>{brief.project.resources.map((r,i)=><p className="path" key={i}>{r}</p>)}<small>引用不代表已读取或取得访问权限。</small></details>}
      <h4>{brief.scope==='project'?'项目成员':'成员身份'}</h4>
      {brief.members.map(a=><p key={a.id}>{a.name} · {roleNames[a.role]} · {a.model_id}</p>)}
      <h4>任务资料（{brief.tasks.length}）</h4>
      {!brief.tasks.length&&<p>暂无对应任务。</p>}
      {brief.tasks.map(t=><details key={t.id}><summary>{t.title} · 草案 · {t.agent_ids.length?'已分配 '+t.agent_ids.length+' 位成员':'未分配'}</summary>
        <h4>目标</h4><p className="prewrap">{t.goal||'未填写'}</p>
        <h4>修改范围</h4><p className="prewrap">{t.scope.join('\n')||'未填写'}</p>
        <h4>验收条件</h4><p className="prewrap">{t.acceptance||'未填写'}</p>
        {brief.task_records.filter(r=>r.task_id===t.id).map(r=><article key={r.id}>
          <small>{r.kind==='progress'?'进展':r.kind==='blocker'?'阻碍':'交接'} · 手动记录 · 未自动核验</small><p className="prewrap">{r.body}</p>
          {r.references.map((ref,i)=><p className="path" key={i}>{ref}</p>)}
          <details><summary>记录时的任务内容</summary><p>{r.task_snapshot.title}</p><p className="prewrap">{r.task_snapshot.goal}</p><p className="prewrap">{r.task_snapshot.scope.join('\n')}</p><p className="prewrap">{r.task_snapshot.acceptance}</p></details>
        </article>)}
      </details>)}
      <p className="muted">任务与手动记录不代表已施工、测试或审查通过。自动派工与工具施工流程尚未接通。</p>
    </>}
  </aside>;
}
