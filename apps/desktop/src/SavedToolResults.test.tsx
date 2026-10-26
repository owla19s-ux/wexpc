// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {cleanup,render,screen,waitFor} from '@testing-library/react';
import {SavedToolResults} from './SavedToolResults';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:String}));
afterEach(()=>{cleanup();ipc.mockReset();});
it('shows saved Cargo cancellation as unknown side effects rather than success',async()=>{
  ipc.mockResolvedValue([{record_id:'cancelled',call_id:'call_2',tool_name:'cargo_test',sequence:2,
    result:JSON.stringify({process:{program:'cargo',working_directory:'C:/project',success:false,exit_code:null,
      elapsed_ms:150,stdout:'before stop',stderr:'',stdout_truncated:false,stderr_truncated:false,termination:'cancelled'}})}]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_1" busy={false}/>);
  expect(await screen.findByText('进程已取消，副作用待核查')).toBeTruthy();
  expect(screen.getByText('before stop')).toBeTruthy();
  expect(screen.queryByText('进程退出成功（未独立核验）')).toBeNull();
});
it('presents actual process failure and truncation as readable saved evidence',async()=>{
  ipc.mockResolvedValue([{record_id:'process',call_id:'call_1',tool_name:'run_command',sequence:1,
    result:JSON.stringify({process:{program:'python',working_directory:'C:/project',success:false,exit_code:17,
      elapsed_ms:123,stdout:'partial stdout',stderr:'test failed',stdout_truncated:true,stderr_truncated:false}})}]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_1" busy={false}/>);
  expect(await screen.findByText('退出码：17')).toBeTruthy();
  expect(screen.getByText('标准输出已截断')).toBeTruthy();
  expect(screen.getByText('test failed')).toBeTruthy();
  expect(ipc).toHaveBeenCalledTimes(1);
});
it('distinguishes durable output order from legacy records with unknown order',async()=>{
  ipc.mockResolvedValue([
    {record_id:'legacy',call_id:'old',tool_name:'read_file',result:'old output'},
    {record_id:'new',call_id:'new',tool_name:'read_file',result:'new output',sequence:1},
  ]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_1" busy={false}/>);
  expect(await screen.findByText(/旧记录：顺序未记录/)).toBeTruthy();
  expect(screen.getByText(/保存顺序 1/)).toBeTruthy();
  expect(ipc).toHaveBeenCalledTimes(1);
});

it('shows the approved request arguments and timeout alongside the saved process result',async()=>{
  ipc.mockResolvedValue([{record_id:'process_args',call_id:'call_2',tool_name:'run_command',sequence:2,
    arguments_json:JSON.stringify({program:'python',arguments:['-m','pytest','tests/my test.py'],timeout_ms:5000}),
    result:JSON.stringify({process:{program:'python',working_directory:'C:/project',timeout_ms:5000,success:false,exit_code:17,
      elapsed_ms:123,stdout:'',stderr:'test failed',stdout_truncated:false,stderr_truncated:false}})}]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_2" busy={false}/>);
  expect(await screen.findByText('请求超时：5000 ms')).toBeTruthy();
  expect(screen.getByText(JSON.stringify(['-m','pytest','tests/my test.py']))).toBeTruthy();
  expect(screen.getByText('退出码：17')).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith('tool_results',{conversationId:'conv_A',turnId:'turn_2'});
  expect(ipc).toHaveBeenCalledTimes(1);
});
it('does not present mismatched proposal arguments as the approved command',async()=>{
  ipc.mockResolvedValue([{record_id:'mismatch',call_id:'call_3',tool_name:'run_command',sequence:3,
    arguments_json:JSON.stringify({program:'other',arguments:['--unsafe'],timeout_ms:5000}),
    result:JSON.stringify({process:{program:'python',working_directory:'C:/project',timeout_ms:5000,success:true,exit_code:0,
      elapsed_ms:20,stdout:'ok',stderr:'',stdout_truncated:false,stderr_truncated:false}})}]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_3" busy={false}/>);
  expect(await screen.findByText('请求参数缺失、无效或与进程结果不一致，无法核对完整命令。')).toBeTruthy();
  expect(screen.queryByText(JSON.stringify(['--unsafe']))).toBeNull();
  expect(ipc).toHaveBeenCalledTimes(1);
});

it('distinguishes incomplete capture from byte-limit truncation in a saved receipt',async()=>{
  ipc.mockResolvedValue([{record_id:'partial',call_id:'call_partial',tool_name:'run_command',sequence:1,
    result:JSON.stringify({process:{program:'tool',working_directory:'C:/project',success:false,exit_code:0,
      elapsed_ms:150,stdout:'received bytes',stderr:'',stdout_truncated:false,stderr_truncated:false,
      stdout_complete:false,stderr_complete:true,termination:'timed_out'}})}]);
  render(<SavedToolResults conversationId="conv_A" turnId="turn_partial" busy={false}/>);
  expect(await screen.findByText('标准输出未完整收集')).toBeTruthy();
  expect(screen.getByText('标准错误已收集至管道关闭')).toBeTruthy();
  expect(screen.queryByText('标准输出已截断')).toBeNull();
});

it('never displays saved evidence from the previous conversation while the next query is pending',async()=>{
  let resolveNext:((rows:unknown[])=>void)|undefined;
  ipc.mockImplementation((_name:string,args:{conversationId:string})=>args.conversationId==='conv_A'
    ?Promise.resolve([{record_id:'old',call_id:'call_old',tool_name:'read_file',result:'private previous conversation',sequence:1}])
    :new Promise(resolve=>{resolveNext=resolve;}));
  const view=render(<SavedToolResults conversationId="conv_A" turnId="turn_1" busy={false}/>);
  expect(await screen.findByText('private previous conversation')).toBeTruthy();
  view.rerender(<SavedToolResults conversationId="conv_B" turnId="turn_2" busy={false}/>);
  expect(screen.queryByText('private previous conversation')).toBeNull();
  resolveNext?.([{record_id:'new',call_id:'call_new',tool_name:'read_file',result:'current conversation evidence',sequence:1}]);
  expect(await screen.findByText('current conversation evidence')).toBeTruthy();
  await waitFor(()=>expect(screen.queryByText('private previous conversation')).toBeNull());
});
