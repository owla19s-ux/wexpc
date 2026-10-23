import { invoke, isTauri } from '@tauri-apps/api/core';
export const desktopAvailable = isTauri();
export function command<T>(name:string, args?:Record<string,unknown>):Promise<T> {
  if (!desktopAvailable) return Promise.reject(new Error('请通过桌面程序打开，浏览器预览未连接本地服务。'));
  return invoke<T>(name,args);
}
export function errorMessage(error:unknown):string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) return String(error.message);
  return typeof error === 'string' ? error : '操作未完成，请重试。';
}
