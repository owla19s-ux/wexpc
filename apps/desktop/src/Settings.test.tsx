// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { Settings } from './Settings';
import type { Conversation, Snapshot } from './types';

vi.mock('./LocalDirectories', () => ({
  LocalDirectories: ({ conversationId }: { conversationId: string }) => <div data-testid="local-scope">{conversationId}</div>,
}));
vi.mock('./GithubToolPolicies', () => ({
  GithubToolPolicies: ({ conversationId }: { conversationId: string }) => <div data-testid="github-scope">{conversationId}</div>,
}));

const a = { id: 'conv-A', title: '普通对话 A', project_id: null } as Conversation;
const b = { id: 'conv-B', title: '项目对话 B', project_id: 'project-B' } as Conversation;
const state = { conversations: [a, b], connections: [{ id: 'api', enabled: true }] } as unknown as Snapshot;
afterEach(cleanup);

it('uses existing Host-backed controls only for the selected conversation', () => {
  const openConversation = vi.fn();
  const openConnections = vi.fn();
  render(<Settings data={state} onChanged={async () => {}} openConversation={openConversation} openConnections={openConnections}/>);
  expect(screen.getByTestId('local-scope').textContent).toBe('conv-A');
  expect(screen.getByTestId('github-scope').textContent).toBe('conv-A');
  fireEvent.change(screen.getByRole('combobox', { name: '选择授权所属对话' }), { target: { value: 'conv-B' } });
  expect(screen.getByTestId('local-scope').textContent).toBe('conv-B');
  expect(screen.getByTestId('github-scope').textContent).toBe('conv-B');
  fireEvent.click(screen.getByRole('button', { name: '进入此对话查看进程与工具回执' }));
  expect(openConversation).toHaveBeenCalledWith(b);
  fireEvent.click(screen.getByRole('button', { name: '管理 API 与 GitHub 连接' }));
  expect(openConnections).toHaveBeenCalledOnce();
});

it('does not imply permissions exist before the authoritative snapshot loads', () => {
  render(<Settings data={null} onChanged={async () => {}} openConversation={vi.fn()} openConnections={vi.fn()}/>);
  expect(screen.getByText(/尚未读取桌面服务的权威数据/)).toBeTruthy();
  expect(screen.queryByTestId('local-scope')).toBeNull();
  expect(screen.queryByTestId('github-scope')).toBeNull();
});

it('does not invent global permissions when no conversation exists', () => {
  render(<Settings data={{ ...state, conversations: [] }} onChanged={async () => {}} openConversation={vi.fn()} openConnections={vi.fn()}/>);
  expect(screen.getByText(/尚无对话/)).toBeTruthy();
  expect(screen.queryByRole('combobox', { name: '选择授权所属对话' })).toBeNull();
});
