// @vitest-environment happy-dom
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {LocalDirectories} from './LocalDirectories';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:(e:unknown)=>e instanceof Error?e.message:String(e)}));
const directory={id:'root_A',path:'/project/A',access:'read',available:true};
const state={revision:3,directories:[directory]};
beforeEach(()=>ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
 if(name==='local_directory_status')return state;
 if(name==='local_read_policy')return {policy:'ask',revision:0};
 if(name==='local_create_policy')return {policy:'ask',revision:0};
 if(name==='set_local_read_policy')return {policy:args.policy,revision:1};
 if(name==='set_local_create_policy')return {policy:args.policy,revision:1};
 if(name==='authorize_local_directory')return {revision:4,directories:[directory,{id:'root_B',path:'/project/B',access:args.access,available:true}]};
 if(name==='edit_local_directory')return {revision:4,directories:[{...directory,access:args.access}]};
 if(name==='revoke_local_directory')return {revision:4,directories:[]};
 throw new Error(name);
}));
afterEach(()=>{cleanup();ipc.mockReset();});
it('saves create policy independently of read policy and directory grants',async()=>{
 const changed=vi.fn().mockResolvedValue(undefined);
 render(<LocalDirectories conversationId="conv_A" onChanged={changed}/>);
 const create=await screen.findByRole('combobox',{name:'本地文件新建策略'});
 expect((create as HTMLSelectElement).value).toBe('ask');
 fireEvent.change(create,{target:{value:'automatic'}});
 await waitFor(()=>expect(ipc).toHaveBeenCalledWith('set_local_create_policy',{conversationId:'conv_A',expectedRevision:0,policy:'automatic'}));
 await waitFor(()=>expect(changed).toHaveBeenCalledOnce());
 expect((create as HTMLSelectElement).value).toBe('automatic');
 expect((screen.getByRole('combobox',{name:'本地文件读取策略'}) as HTMLSelectElement).value).toBe('ask');
 expect(ipc.mock.calls.some(([n])=>n==='authorize_local_directory'||n==='edit_local_directory'||n==='set_local_read_policy')).toBe(false);
});
it('adds another directory through native selection with independent read/write scope',async()=>{
 render(<LocalDirectories conversationId="conv_A" onChanged={async()=>{}}/>);
 await screen.findByText('/project/A');fireEvent.change(screen.getByRole('combobox',{name:'新目录权限'}),{target:{value:'read_write'}});
 fireEvent.click(screen.getByRole('button',{name:'选择并授权目录'}));await screen.findByText('/project/B');
 expect(ipc).toHaveBeenCalledWith('authorize_local_directory',{conversationId:'conv_A',expectedRevision:3,access:'read_write'});
 expect(ipc.mock.calls.some(([n])=>n==='configure_github'||n==='set_github_enabled'||n==='send_message')).toBe(false);
});
it('edits and revokes current scope with the updated authorization revision',async()=>{
 render(<LocalDirectories conversationId="conv_A" onChanged={async()=>{}}/>);
 const select=await screen.findByRole('combobox',{name:'目录权限 /project/A'});
 fireEvent.change(select,{target:{value:'write'}});
 await waitFor(()=>expect((select as HTMLSelectElement).value).toBe('write'));
 fireEvent.click(screen.getByRole('button',{name:'撤销 /project/A'}));
 await screen.findByText('尚未授权目录。');
 expect(ipc).toHaveBeenCalledWith('edit_local_directory',{conversationId:'conv_A',expectedRevision:3,directoryId:'root_A',access:'write'});
 expect(ipc).toHaveBeenCalledWith('revoke_local_directory',{conversationId:'conv_A',expectedRevision:4,directoryId:'root_A'});
});
it('keeps prior scope on conflict and displays failure without retrying',async()=>{
 ipc.mockImplementation(async(name:string)=>{if(name==='local_directory_status')return state;if(name==='local_read_policy')return {policy:'ask',revision:0};throw new Error('授权已变化，请刷新');});
 render(<LocalDirectories conversationId="conv_A" onChanged={async()=>{}}/>);
 const select=await screen.findByRole('combobox',{name:'目录权限 /project/A'});fireEvent.change(select,{target:{value:'write'}});
 await screen.findByText('授权已变化，请刷新');expect((select as HTMLSelectElement).value).toBe('read');
 expect(ipc.mock.calls.filter(([n])=>n==='edit_local_directory')).toHaveLength(1);
});
it('saves local automatic policy independently of directory grants and reports unavailable roots',async()=>{
 ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
 if(name==='local_directory_status')return {revision:3,directories:[{...directory,available:false}]};
 if(name==='local_read_policy')return {policy:'ask',revision:0};return {policy:args.policy,revision:1};
 });const changed=vi.fn().mockResolvedValue(undefined);
 render(<LocalDirectories conversationId="conv_A" onChanged={changed}/>);
 await screen.findByText('目录身份无法核对，请核查位置或撤销后重新选择。');
 fireEvent.change(await screen.findByRole('combobox',{name:'本地文件读取策略'}),{target:{value:'automatic'}});
 await waitFor(()=>expect(changed).toHaveBeenCalledOnce());
 expect(ipc).toHaveBeenCalledWith('set_local_read_policy',{conversationId:'conv_A',expectedRevision:0,policy:'automatic'});
 expect(ipc.mock.calls.some(([n])=>n==='authorize_local_directory'||n==='edit_local_directory')).toBe(false);
});
it('does not show a late directory save in another conversation',async()=>{
 let release:((v:unknown)=>void)|undefined;
 ipc.mockImplementation(async(name:string,args:Record<string,unknown>)=>{
 if(name==='local_directory_status')return args.conversationId==='conv_B'?{revision:0,directories:[]}:state;
 if(name==='local_read_policy')return {policy:'ask',revision:0};return new Promise(resolve=>{release=resolve;});
 });
 const view=render(<LocalDirectories key="A" conversationId="conv_A" onChanged={async()=>{}}/>);
 await screen.findByText('/project/A');fireEvent.click(screen.getByRole('button',{name:'选择并授权目录'}));
 await waitFor(()=>expect(release).toBeDefined());
 view.rerender(<LocalDirectories key="B" conversationId="conv_B" onChanged={async()=>{}}/>);
 await screen.findByText('尚未授权目录。');await act(async()=>{release?.({revision:4,directories:[directory]});});
 expect(screen.queryByText('/project/A')).toBeNull();
});
