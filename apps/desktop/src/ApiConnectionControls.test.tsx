// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { command } from './client';
import { ApiConnectionControls } from './Connections';
import type { AiConnection } from './types';
import type { Run } from './App';

vi.mock('./client', () => ({ command: vi.fn() }));
const mockedCommand = command as unknown as ReturnType<typeof vi.fn>;
const connection = {
  id: 'api-a', name: 'API A', base_url: 'https://example.invalid',
  models: ['model-a'], enabled: true,
} as AiConnection;
const run: Run = async action => { await action(); };
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it('does not invert a cached API connection state while Host snapshot is stale', () => {
  render(<ApiConnectionControls connections={[connection]} snapshotFresh={false} run={run}/>);
  const toggle = screen.getByRole('button', {name: '停用'}) as HTMLButtonElement;
  expect(toggle.disabled).toBe(true);
  expect(screen.getByRole('status').textContent).toMatch(/未核实的快照/);
  fireEvent.click(toggle);
  expect(mockedCommand).not.toHaveBeenCalled();
});

it('uses the verified connection state for an enabled toggle', async () => {
  mockedCommand.mockResolvedValue(undefined);
  render(<ApiConnectionControls connections={[connection]} snapshotFresh={true} run={run}/>);
  fireEvent.click(screen.getByRole('button', {name: '停用'}));
  await waitFor(() => expect(mockedCommand).toHaveBeenCalledWith(
    'set_connection_enabled', {connectionId: 'api-a', enabled: false},
  ));
});

it('blocks duplicate toggles while the first Host mutation is pending', async () => {
  let finish: () => void = () => {};
  mockedCommand.mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  render(<ApiConnectionControls connections={[connection]} snapshotFresh={true} run={run}/>);
  const toggle = screen.getByRole('button', {name: '停用'}) as HTMLButtonElement;
  fireEvent.click(toggle);
  expect(toggle.disabled).toBe(true);
  fireEvent.click(toggle);
  expect(mockedCommand).toHaveBeenCalledTimes(1);
  finish();
  await waitFor(() => expect(toggle.disabled).toBe(false));
});
