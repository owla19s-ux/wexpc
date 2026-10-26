// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {GithubConnection} from './Connections';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:(e:unknown)=>e instanceof Error?e.message:String(e)}));
const run=async(action:()=>Promise<void>)=>{await action();};
afterEach(()=>{cleanup();ipc.mockReset();});

it('does not report credentials absent when the first status query fails',async()=>{
  let unavailable=true;
  ipc.mockImplementation(async(name:string)=>{
    if(name==='github_status'){
      if(unavailable)throw new Error('凭证状态暂不可读');
      return {configured:false,enabled:false};
    }
    throw new Error(name);
  });
  render(<GithubConnection run={run}/>);
  await screen.findByRole('alert');
  expect(screen.getByText('GitHub 状态尚未核实，不能判断是否配置凭证。')).toBeTruthy();
  expect(screen.queryByText('尚未配置 GitHub 凭证')).toBeNull();
  unavailable=false;
  fireEvent.click(screen.getByRole('button',{name:'重新查询 GitHub 状态'}));
  await screen.findByText('尚未配置 GitHub 凭证');
  expect(screen.queryByRole('alert')).toBeNull();
});

it('keeps a failed refresh visibly stale and blocks changes until reverified',async()=>{
  let unavailable=false;
  let enabled=true;
  ipc.mockImplementation(async(name:string,args?:{enabled:boolean})=>{
    if(name==='github_status'){
      if(unavailable)throw new Error('查询中断');
      return {configured:true,enabled};
    }
    if(name==='set_github_enabled'){enabled=Boolean(args?.enabled);return;}
    if(name==='revoke_github')throw new Error('不应撤销');
    throw new Error(name);
  });
  render(<GithubConnection run={run}/>);
  await screen.findByText('GitHub 已启用（按各对话执行策略）');
  unavailable=true;
  fireEvent.click(screen.getByRole('button',{name:'重新查询 GitHub 状态'}));
  await screen.findByRole('alert');
  expect(screen.getByText('上次核对：GitHub 已启用（按各对话执行策略）；当前状态尚未核实。')).toBeTruthy();
  const toggle=screen.getByRole('button',{name:'关闭 GitHub 全局访问'}) as HTMLButtonElement;
  const revoke=screen.getByRole('button',{name:'删除本机 GitHub 凭证并关闭访问'}) as HTMLButtonElement;
  expect(toggle.disabled).toBe(true);
  expect(revoke.disabled).toBe(true);
  fireEvent.click(toggle);fireEvent.click(revoke);
  expect(ipc.mock.calls.filter(([name])=>name==='set_github_enabled'||name==='revoke_github')).toHaveLength(0);
  unavailable=false;
  fireEvent.click(screen.getByRole('button',{name:'重新查询 GitHub 状态'}));
  await waitFor(()=>expect(toggle.disabled).toBe(false));
  fireEvent.click(toggle);
  await screen.findByText('GitHub 已配置，尚未启用');
  expect(ipc).toHaveBeenCalledWith('set_github_enabled',{enabled:false});
});

it('ignores an older status reply after credential replacement was rechecked',async()=>{
  let releaseFirst:((status:{configured:boolean;enabled:boolean})=>void)|undefined;
  let statusReads=0;
  ipc.mockImplementation((name:string)=>{
    if(name==='github_status'){
      statusReads++;
      if(statusReads===1)return new Promise(resolve=>{releaseFirst=resolve;});
      return Promise.resolve({configured:true,enabled:false});
    }
    if(name==='configure_github')return Promise.resolve();
    throw new Error(name);
  });
  render(<GithubConnection run={run}/>);
  fireEvent.change(screen.getByLabelText('GitHub Personal Access Token'),{target:{value:'test-token'}});
  fireEvent.click(screen.getByRole('button',{name:'保存 GitHub 凭证'}));
  await screen.findByText('GitHub 已配置，尚未启用');
  await act(async()=>{releaseFirst?.({configured:false,enabled:false});});
  expect(screen.getByText('GitHub 已配置，尚未启用')).toBeTruthy();
  expect(screen.queryByText('尚未配置 GitHub 凭证')).toBeNull();
});
