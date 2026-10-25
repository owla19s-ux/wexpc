// @vitest-environment happy-dom
import {afterEach, expect, it, vi} from 'vitest';
import {cleanup, fireEvent, render, screen} from '@testing-library/react';
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

it('keeps the last successful receipt on refresh failure without leaking it to another conversation',async()=>{
  let reads=0;
  ipc.mockImplementation(async(name:string,args:{conversationId:string})=>{
    if(name!=='tool_receipts')throw new Error(name);
    if(args.conversationId==='conv_B')throw new Error('对话 B 读取失败');
    reads++;
    if(reads>1)throw new Error('临时读取失败');
    return [{
      operation_id:'op_1',attempt_id:'attempt_1',state:'succeeded',
      turn_id:'turn_1',content_digest:null,
      policy:{capability_id:'local_file_create',policy:'ask',revision:3},
    }];
  });
  const view=render(<ToolReceipts conversationId="conv_A" busy={false}/>);
  await screen.findByText('执行成功（未独立核验）');
  fireEvent.click(screen.getByText('刷新执行记录'));
  await screen.findByText(/刷新失败，以下仍为上次读取的记录：临时读取失败/);
  expect(screen.getByText('执行成功（未独立核验）')).toBeTruthy();
  expect(screen.getByText('工具执行记录（1）')).toBeTruthy();
  view.rerender(<ToolReceipts conversationId="conv_B" busy={false}/>);
  await screen.findByText('对话 B 读取失败');
  expect(screen.queryByText('执行成功（未独立核验）')).toBeNull();
  expect(screen.getByText('工具执行记录（0）')).toBeTruthy();
});
