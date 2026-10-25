// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {cleanup,render,screen} from '@testing-library/react';
import {SavedToolResults} from './SavedToolResults';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:String}));
afterEach(()=>{cleanup();ipc.mockReset();});
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
