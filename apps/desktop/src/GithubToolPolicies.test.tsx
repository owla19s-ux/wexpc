// @vitest-environment happy-dom
import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {GithubToolPolicies} from './GithubToolPolicies';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:(error:unknown)=>error instanceof Error?error.message:String(error)}));
type Row={conversation_id:string;capability_id:string;policy:string;revision:number};
function rows(conversation:string):Row[]{return ['github_file_read','github_file_create','github_file_write'].map(capability_id=>({conversation_id:conversation,capability_id,policy:'ask',revision:0}));}
beforeEach(()=>{
 ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
  if(name==='github_tool_policies')return rows(String(args.conversationId));
  if(name==='github_status')return {configured:true,enabled:false};
  if(name==='set_github_tool_policy')return {conversation_id:args.conversationId,capability_id:args.capabilityId,policy:args.policy,revision:Number(args.expectedRevision)+1};
  throw new Error('Unexpected IPC: '+name);
 });
});
afterEach(()=>{cleanup();ipc.mockReset();});
describe('conversation GitHub policy controls (real React DOM, mocked IPC)',()=>{
 it('saves an automatic preference with the exact scope without enabling any connection',async()=>{
  const onChanged=vi.fn().mockResolvedValue(undefined);
  render(<GithubToolPolicies conversationId="conv_A" onChanged={onChanged}/>);
  const read=await screen.findByRole('combobox',{name:/读取文件/});
  expect((read as HTMLSelectElement).value).toBe('ask');
  fireEvent.change(read,{target:{value:'automatic'}});
  await waitFor(()=>expect(ipc).toHaveBeenCalledWith('set_github_tool_policy',{conversationId:'conv_A',capabilityId:'github_file_read',expectedRevision:0,policy:'automatic'}));
  await waitFor(()=>expect(onChanged).toHaveBeenCalledTimes(1));
  expect((read as HTMLSelectElement).value).toBe('automatic');
  expect(screen.getByText('GitHub 总开关已关闭')).toBeTruthy();
  expect(ipc.mock.calls.some(([name])=>name==='set_github_enabled'||name==='configure_github'||name==='send_message')).toBe(false);
 });
 it('preserves the displayed old policy on conflict and only reloads after an explicit refresh',async()=>{
  let latest=false;const onChanged=vi.fn().mockResolvedValue(undefined);
  ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
   if(name==='github_status')return {configured:true,enabled:true};
   if(name==='github_tool_policies')return rows(String(args.conversationId)).map(row=>latest&&row.capability_id==='github_file_read'?{...row,policy:'block',revision:2}:row);
   throw new Error('策略已变化，请刷新后核对');
  });
  render(<GithubToolPolicies conversationId="conv_A" onChanged={onChanged}/>);
  const read=await screen.findByRole('combobox',{name:/读取文件/});fireEvent.change(read,{target:{value:'automatic'}});
  await screen.findByText('策略已变化，请刷新后核对');
  expect((read as HTMLSelectElement).value).toBe('ask');expect(onChanged).not.toHaveBeenCalled();
  expect(ipc.mock.calls.filter(([name])=>name==='set_github_tool_policy')).toHaveLength(1);
  latest=true;fireEvent.click(screen.getByRole('button',{name:'刷新策略和连接状态'}));
  await waitFor(()=>expect((screen.getByRole('combobox',{name:/读取文件/}) as HTMLSelectElement).value).toBe('block'));
 });
 it('keeps a successfully saved policy when the parent snapshot refresh fails',async()=>{
  const onChanged=vi.fn().mockRejectedValue(new Error('snapshot offline'));
  render(<GithubToolPolicies conversationId="conv_A" onChanged={onChanged}/>);
  const write=await screen.findByRole('combobox',{name:/更新文件/});fireEvent.change(write,{target:{value:'automatic'}});
  await screen.findByText('策略已保存，但项目状态刷新失败：snapshot offline');
  expect((write as HTMLSelectElement).value).toBe('automatic');
  expect((screen.getByRole('combobox',{name:/读取文件/}) as HTMLSelectElement).value).toBe('ask');
 });
 it('does not display a late save from a previous conversation in the new conversation',async()=>{
  let release:((row:Row)=>void)|undefined;const onChanged=vi.fn().mockResolvedValue(undefined);
  ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
   if(name==='github_status')return {configured:true,enabled:true};
   if(name==='github_tool_policies')return rows(String(args.conversationId));
   return new Promise<Row>(resolve=>{release=resolve;});
  });
  const view=render(<GithubToolPolicies key="conv_A" conversationId="conv_A" onChanged={onChanged}/>);
  fireEvent.change(await screen.findByRole('combobox',{name:/读取文件/}),{target:{value:'automatic'}});
  await waitFor(()=>expect(release).toBeDefined());
  view.rerender(<GithubToolPolicies key="conv_B" conversationId="conv_B" onChanged={onChanged}/>);
  await screen.findByRole('combobox',{name:/读取文件/});
  await act(async()=>{release?.({conversation_id:'conv_A',capability_id:'github_file_read',policy:'automatic',revision:1});});
  expect((screen.getByRole('combobox',{name:/读取文件/}) as HTMLSelectElement).value).toBe('ask');
  expect(ipc).toHaveBeenCalledWith('github_tool_policies',{conversationId:'conv_B'});
 });
});
