import { useState } from 'react';
import { GithubToolPolicies } from './GithubToolPolicies';
import { LocalDirectories } from './LocalDirectories';
import type { Conversation, Snapshot } from './types';

type SettingsProps = {
  data: Snapshot | null;
  snapshotFresh: boolean;
  onChanged: () => Promise<void>;
  openConversation: (conversation: Conversation) => void;
  openConnections: () => void;
};

// Permission truth remains in the Host. This page only selects a conversation
// scope and reuses its existing revision-checked authorization controls.
export function Settings({ data, snapshotFresh, onChanged, openConversation, openConnections }: SettingsProps) {
  const [selectedId, setSelectedId] = useState('');
  const conversations = data?.conversations ?? [];
  // Permission controls require an explicit conversation selection. A changed
  // snapshot order must never silently move their effective authorization scope.
  const selected = selectedId ? conversations.find(c => c.id === selectedId) : undefined;
  // A deleted conversation must not silently redirect permission controls to another scope.
  const missingSelection = Boolean(selectedId && !selected);
  const enabled = data?.connections.filter(c => c.enabled).length ?? 0;

  return <section>
    <h1>设置与权限</h1>
    <h2>连接与凭证</h2>
    <p>{data ? `${snapshotFresh ? '最近一次核实' : '上次读取（尚未核实）'}：${data.connections.length} 个 API 连接，${enabled} 个已启用。` : '连接状态尚未核实。'} API Key 与 GitHub Token 不在此页展示。</p>
    <button type="button" onClick={openConnections}>管理 API 与 GitHub 连接</button>

    <h2>对话授权与执行策略</h2>
    {data && !snapshotFresh && <p className="notice" role="status">对话列表来自上次成功快照；实际目录和工具策略仍需由 Host 分别核实。</p>}
    <p className="muted">目录授权和 GitHub 工具策略按对话分别保存；项目、Space、模型或岗位关联不会自动授权。此处没有全局文件或终端授权开关。</p>
    {conversations.length ? <>
      <label>选择授权所属对话
        <select aria-label="选择授权所属对话" value={selected?.id ?? ''} onChange={event => setSelectedId(event.target.value)}>
          {!selected && <option value="" disabled>{missingSelection ? "请重新选择对话" : "请先选择对话"}</option>}
          {conversations.map(conversation => <option key={conversation.id} value={conversation.id}>{conversation.title}</option>)}
        </select>
      </label>
      {missingSelection && <p className="notice" role="alert">此前选择的对话已不在当前列表中。请重新选择授权所属对话；不会自动切换到其他对话。</p>}
      {selected && <>
        <p>当前授权范围：{selected.title}（{selected.project_id ? '项目工作对话' : '普通对话'}）</p>
        <LocalDirectories key={'local_' + selected.id} conversationId={selected.id} onChanged={onChanged}/>
        <GithubToolPolicies key={'github_' + selected.id} conversationId={selected.id} onChanged={onChanged}/>
        <button type="button" onClick={() => openConversation(selected)}>进入此对话查看进程与工具回执</button>
      </>}
    </> : <p>{data ? '尚无对话。请先创建普通对话或项目工作对话，再配置对应授权。' : '尚未读取桌面服务的权威数据，不能确认授权状态。'}</p>}

    <h2>终端、环境与本机资料</h2>
    <p className="muted">本地命令目前仍逐次审批；持续终端授权、环境发现与选择、全局权限总览及安装/数据目录迁移尚未接通。这里不提供无效的授权或迁移按钮。</p>
    <p className="muted">重装系统或卸载重装后，不应凭残留资料自动恢复凭证、OS 权限或工具授权。Windows 实机安装与恢复尚未验收。</p>
  </section>;
}
