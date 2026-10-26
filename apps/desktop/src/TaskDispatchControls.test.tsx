// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {TaskDispatchControls} from './TaskDispatchControls';
import type {Agent,Task} from './types';

const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc}));
const run=async(action:()=>Promise<void>)=>{await action();};
const agents:Agent[]=[
  {id:'agent-a',project_id:'project',name:'甲',role:'worker',connection_id:'conn',model_id:'model',conversation_id:'ca'},
  {id:'agent-b',project_id:'project',name:'乙',role:'reviewer',connection_id:'conn',model_id:'model',conversation_id:'cb'},
];
const task:Task={id:'task-1',project_id:'project',title:'任务',goal:'',scope:['src'],acceptance:'测试通过',
  agent_ids:['agent-a','agent-b'],status:'published',publication_epoch:2};
afterEach(()=>{cleanup();ipc.mockReset();});

it('requires an explicit assignee and sends the chosen agent to preview and queue',async()=>{
  ipc.mockImplementation(async(name:string,args:{taskId:string;agentId:string})=>{
    if(name==='preview_task_dispatch')return {taskId:args.taskId,agentId:args.agentId,projectId:'project',assignmentEpoch:2};
    if(name==='queue_dispatch_attempt')return {id:'attempt'};
    throw new Error(name);
  });
  render(<TaskDispatchControls task={task} agents={agents} revision={11} attempts={[]} run={run}/>);
  const preview=screen.getByRole('button',{name:'核对调度准入（不执行）'}) as HTMLButtonElement;
  const queue=screen.getByRole('button',{name:'登记待派工尝试（不启动工具）'}) as HTMLButtonElement;
  expect(preview.disabled).toBe(true);
  expect(queue.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-b'}});
  fireEvent.click(preview);
  await screen.findByText(/仅通过准入预检：agent-b/);
  fireEvent.click(queue);
  await waitFor(()=>expect(ipc).toHaveBeenCalledWith('queue_dispatch_attempt',
    {revision:11,taskId:'task-1',agentId:'agent-b'}));
  expect(ipc).toHaveBeenCalledWith('preview_task_dispatch',
    {revision:11,taskId:'task-1',agentId:'agent-b'});
  expect(ipc.mock.calls.some(([name])=>name==='run_command'||name==='send_message')).toBe(false);
});

it('does not show a late preview for a previously selected agent',async()=>{
  let resolveOld:((v:unknown)=>void)|undefined;
  ipc.mockImplementation((name:string,args:{agentId:string;taskId:string})=>{
    if(name==='preview_task_dispatch'&&args.agentId==='agent-b')
      return new Promise(resolve=>{resolveOld=resolve;});
    if(name==='preview_task_dispatch')return Promise.resolve({
      taskId:args.taskId,agentId:args.agentId,projectId:'project',assignmentEpoch:2});
    throw new Error(name);
  });
  render(<TaskDispatchControls task={task} agents={agents} revision={11} attempts={[]} run={run}/>);
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-b'}});
  fireEvent.click(screen.getByRole('button',{name:'核对调度准入（不执行）'}));
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-a'}});
  await act(async()=>{resolveOld?.({taskId:'task-1',agentId:'agent-b',assignmentEpoch:2});});
  expect(screen.queryByRole('status')).toBeNull();
  fireEvent.click(screen.getByRole('button',{name:'核对调度准入（不执行）'}));
  await screen.findByText(/仅通过准入预检：agent-a/);
});

it('invalidates a preview when snapshot revision or task changes',async()=>{
  ipc.mockImplementation(async(name:string,args:{taskId:string;agentId:string})=>{
    if(name==='preview_task_dispatch')return {taskId:args.taskId,agentId:args.agentId,assignmentEpoch:2};
    throw new Error(name);
  });
  const view=render(<TaskDispatchControls task={task} agents={agents} revision={11} attempts={[]} run={run}/>);
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-a'}});
  fireEvent.click(screen.getByRole('button',{name:'核对调度准入（不执行）'}));
  await screen.findByRole('status');
  view.rerender(<TaskDispatchControls task={task} agents={agents} revision={12} attempts={[]} run={run}/>);
  await waitFor(()=>expect(screen.queryByRole('status')).toBeNull());
  view.rerender(<TaskDispatchControls task={{...task,id:'task-2'}} agents={agents} revision={12} attempts={[]} run={run}/>);
  await waitFor(()=>expect((screen.getByLabelText('选择派工成员') as HTMLSelectElement).value).toBe(''));
});

it('shows the persisted assignee and fences repeated dispatch when the outcome is unknown',()=>{
  render(<TaskDispatchControls task={task} agents={agents} revision={11} run={run} attempts={[{
    id:'attempt-b',task_id:'task-1',project_id:'project',agent_id:'agent-b',
    publication_epoch:2,status:'outcome_unknown',recorded_revision:10,
  }]}/>);
  expect(screen.getByText(/成员 乙（agent-b）/)).toBeTruthy();
  expect(screen.getByText(/结果未知（须核查，禁止自动重试）/)).toBeTruthy();
  expect(screen.getByText(/当前发布版本已登记派工尝试，不能重复登记/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-a'}});
  const queue=screen.getByRole('button',{name:'登记待派工尝试（不启动工具）'}) as HTMLButtonElement;
  expect(queue.disabled).toBe(true);
  fireEvent.click(queue);
  expect(ipc).not.toHaveBeenCalled();
});

it('shows historical dispatch evidence without blocking a new publication epoch',()=>{
  render(<TaskDispatchControls task={{...task,publication_epoch:3}} agents={agents} revision={12} run={run} attempts={[{
    id:'attempt-old',task_id:'task-1',project_id:'project',agent_id:'agent-a',
    publication_epoch:2,status:'pending',recorded_revision:10,
  }]}/>);
  expect(screen.getByText(/成员 甲（agent-a）/)).toBeTruthy();
  expect(screen.getByText(/待领取（尚未执行）/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText('选择派工成员'),{target:{value:'agent-b'}});
  const queue=screen.getByRole('button',{name:'登记待派工尝试（不启动工具）'}) as HTMLButtonElement;
  expect(queue.disabled).toBe(false);
});
