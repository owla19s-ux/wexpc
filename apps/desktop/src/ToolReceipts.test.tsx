// @vitest-environment happy-dom
import {afterEach, expect, it, vi} from 'vitest';
import {cleanup, render, screen} from '@testing-library/react';
import {ToolReceipts} from './ToolReceipts';

const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({
  command:ipc,
  errorMessage:(error:unknown)=>error instanceof Error?error.message:String(error),
}));

afterEach(()=>{cleanup();ipc.mockReset();});

it('shows the local file creation policy in a saved receipt without executing a tool',async()=>{
  ipc.mockImplementation(async(name:string)=>{
    if(name==='tool_receipts')return [{
      operation_id:'op_1',
      attempt_id:'attempt_1',
      state:'succeeded',
      turn_id:'turn_1',
      content_digest:'sha256:example',
      policy:{capability_id:'local_file_create',policy:'ask',revision:3},
    }];
    throw new Error(name);
  });
  render(<ToolReceipts conversationId="conv_A" busy={false}/>);
  expect(await screen.findByText(/执行策略：本地文件新建 · 询问后执行 · 修订 3/)).toBeTruthy();
  expect(screen.getByText('执行成功（未独立核验）')).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith('tool_receipts',{conversationId:'conv_A'});
  expect(ipc).toHaveBeenCalledTimes(1);
});
