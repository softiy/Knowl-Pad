import { invoke } from '@tauri-apps/api/core';
import { asKpError } from './errors';

/** 全项目唯一的 invoke 出口：所有 feature 必须经此调用（PRD FE-04）。 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    throw asKpError(err);
  }
}

export async function callVoid(command: string, args?: Record<string, unknown>): Promise<void> {
  await call<null>(command, args);
}
