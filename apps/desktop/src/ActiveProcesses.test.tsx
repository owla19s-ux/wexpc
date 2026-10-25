// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {ActiveProcesses} from './ActiveProcesses';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:(e:unknown)=>e instanceof Error?e.message:String(e)}));
afterEach(()=>{cleanup();ipc.mockReset();});
const row={conversation_id:'conv_A',attempt_id:'attempt_A',operation_id:'op_A',program:'python',arguments:['-m','pytest'],elapsed_ms:100,cancellation_requested:false};
it('requests cancellation only for the displayed conversation and attempt',async()=>{
 let cancelled=false;
 ipc.mockImplementation(async(name:string)=>{
  if(name==='active_processes')return [{...row,cancellation_requested:cancelled}];
  if(name==='cancel_process'){cancelled=true;return;}
  throw new Error(name);
 });
 render(<ActiveProcesses conversationId="conv_A" busy/>);
 fireEvent.click(await screen.findByRole('button',{name:'取消此进程'}));
 await screen.findByText('已请求取消，等待执行器回收并保存回执。');
 expect(ipc).toHaveBeenCalledWith('cancel_process',{conversationId:'conv_A',attemptId:'attempt_A'});
 expect(screen.queryByRole('button',{name:'取消此进程'})).toBeNull();
});
it('shows cancellation failure without retrying or claiming the process stopped',async()=>{
 ipc.mockImplementation(async(name:string)=>{if(name==='active_processes')return [row];throw new Error('取消请求失败');});
 render(<ActiveProcesses conversationId="conv_A" busy/>);
 fireEvent.click(await screen.findByRole('button',{name:'取消此进程'}));
 await screen.findByText('取消请求失败');
 expect(ipc.mock.calls.filter(([name])=>name==='cancel_process')).toHaveLength(1);
 expect(screen.queryByText('已请求取消，等待执行器回收并保存回执。')).toBeNull();
 await act(async()=>{await new Promise(resolve=>setTimeout(resolve,1100));});
 expect(screen.getByText('取消请求失败')).toBeTruthy();
});
it('does not display the previous conversation process while the new query fails',async()=>{
 ipc.mockImplementation(async(name:string,args:{conversationId:string})=>{if(name==='active_processes'&&args.conversationId==='conv_A')return [row];throw new Error('读取失败');});
 const view=render(<ActiveProcesses conversationId="conv_A" busy/>);
 await screen.findByRole('button',{name:'取消此进程'});
 view.rerender(<ActiveProcesses conversationId="conv_B" busy/>);
 await waitFor(()=>expect(screen.queryByRole('button',{name:'取消此进程'})).toBeNull());
 await screen.findByText('读取失败');
});
