// @vitest-environment happy-dom
import {afterEach,expect,it} from 'vitest';
import {cleanup,render,screen} from '@testing-library/react';
import {ProcessOutput} from './ProcessOutput';

afterEach(cleanup);

const base={
  program:'python',working_directory:'C:/project',success:true,exit_code:0,
  elapsed_ms:10,stdout:'output',stderr:'',stdout_truncated:false,stderr_truncated:false,
};

it('does not silently treat missing legacy capture fields as complete',()=>{
  render(<ProcessOutput body={JSON.stringify({process:base})}/>);
  expect(screen.getByText('标准输出完整性未记录，不能推定已读取至管道关闭。')).toBeTruthy();
  expect(screen.getByText('标准错误完整性未记录，不能推定已读取至管道关闭。')).toBeTruthy();
  expect(screen.queryByText('标准输出已收集至管道关闭')).toBeNull();
  expect(screen.getByText('output')).toBeTruthy();
});

it('keeps truncation separate from unknown completeness',()=>{
  render(<ProcessOutput body={JSON.stringify({process:{...base,stdout_truncated:true,stdout_complete:undefined,stderr_complete:true}})}/>);
  expect(screen.getByText('标准输出已截断')).toBeTruthy();
  expect(screen.getByText('标准输出完整性未记录，不能推定已读取至管道关闭。')).toBeTruthy();
  expect(screen.getByText('标准错误已收集至管道关闭')).toBeTruthy();
  expect(screen.queryByText('标准错误完整性未记录，不能推定已读取至管道关闭。')).toBeNull();
});

it('shows explicitly incomplete capture without a legacy warning',()=>{
  render(<ProcessOutput body={JSON.stringify({process:{...base,stdout_complete:false,stderr_complete:true}})}/>);
  expect(screen.getByText('标准输出未完整收集')).toBeTruthy();
  expect(screen.getByText('标准错误已收集至管道关闭')).toBeTruthy();
  expect(screen.queryByText('标准输出完整性未记录，不能推定已读取至管道关闭。')).toBeNull();
});
