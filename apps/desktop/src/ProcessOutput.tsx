type Report={program:string;working_directory:string;success:boolean;exit_code:number|null;elapsed_ms:number;
  stdout:string;stderr:string;stdout_truncated:boolean;stderr_truncated:boolean;termination?:string};
function decode(body:string):Report|null {
  try {
    const p=JSON.parse(body)?.process;
    if(!p||typeof p!=='object'||!['program','working_directory','stdout','stderr'].every(k=>typeof p[k]==='string')
      ||!['success','stdout_truncated','stderr_truncated'].every(k=>typeof p[k]==='boolean')
      ||!(p.exit_code===null||Number.isInteger(p.exit_code))||!Number.isFinite(p.elapsed_ms))return null;
    return p;
  }catch{return null;}
}
export function ProcessOutput({body}:{body:string}) {
  const report=decode(body);
  if(!report)return <pre className="prewrap">{body}</pre>;
  return <section aria-label="已保存的进程结果">
    <p>{report.termination==='cancelled'?'进程已取消，副作用待核查':report.termination==='timed_out'?'进程超时，副作用待核查':report.termination==='output_interrupted'?'输出中断，副作用待核查':report.success?'进程退出成功（未独立核验）':'进程退出失败'}</p>
    <p>程序：{report.program}</p><p>工作目录：{report.working_directory}</p>
    <p>退出码：{report.exit_code??'未取得数值退出码'}</p><p>耗时：{report.elapsed_ms} ms</p>
    <p>标准输出{report.stdout_truncated?'已截断':''}</p><pre className="prewrap">{report.stdout||'（空）'}</pre>
    <p>标准错误{report.stderr_truncated?'已截断':''}</p><pre className="prewrap">{report.stderr||'（空）'}</pre>
    <p className="muted">这是已保存的执行结果；截断不改变退出状态，也不代表独立验证通过。</p>
  </section>;
}
