// @vitest-environment happy-dom
import {afterEach,expect,it,vi} from 'vitest';
import {cleanup,render,screen} from '@testing-library/react';
import {SavedToolResults} from './SavedToolResults';
const {ipc}=vi.hoisted(()=>({ipc:vi.fn()}));
vi.mock('./client',()=>({command:ipc,errorMessage:String}));
afterEach(()=>{cleanup();ipc.mockReset();});
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
