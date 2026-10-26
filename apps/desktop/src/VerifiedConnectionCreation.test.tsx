// @vitest-environment happy-dom
import { afterEach, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { VerifiedConnectionCreation } from './Connections';
import type { AiConnection } from './types';

const connection: AiConnection = {
  id: 'api-a', name: 'API A', base_url: 'https://example.invalid',
  models: ['model-a'], enabled: true,
};
afterEach(cleanup);

it('blocks both ordinary conversation and project Agent creation with stale Host evidence', () => {
  for (const label of ['创建对话', '添加成员']) {
    const view = render(<VerifiedConnectionCreation connections={[connection]} snapshotFresh={false} label={label}/>);
    expect((screen.getByRole('button', {name: label}) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole('status').textContent).toMatch(/状态尚未核实/);
    expect((screen.getByRole('combobox', {name: 'API 连接'}) as HTMLSelectElement).value).toBe('');
    view.unmount();
  }
});

it('restores the verified connection and model after the latest Host snapshot succeeds', () => {
  const view = render(<VerifiedConnectionCreation connections={[connection]} snapshotFresh={false} label="创建对话"/>);
  view.rerender(<VerifiedConnectionCreation connections={[connection]} snapshotFresh={true} label="创建对话"/>);
  expect((screen.getByRole('button', {name: '创建对话'}) as HTMLButtonElement).disabled).toBe(false);
  expect((screen.getByRole('combobox', {name: 'API 连接'}) as HTMLSelectElement).value).toBe('api-a');
  expect((screen.getByRole('combobox', {name: '模型'}) as HTMLSelectElement).value).toBe('model-a');
  expect(screen.queryByRole('status')).toBeNull();
});

it('does not allow creation when the verified snapshot has no enabled API connection', () => {
  const disabledConnection = {...connection, enabled: false};
  render(<VerifiedConnectionCreation connections={[disabledConnection]} snapshotFresh={true} label="添加成员"/>);
  expect((screen.getByRole('button', {name: '添加成员'}) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/请先在连接与服务中配置并启用 API/)).toBeTruthy();
});
