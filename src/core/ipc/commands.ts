import { call, callVoid } from './client';

export interface VaultSummary {
  id: number;
  absPath: string;
  displayName: string;
  lastOpened?: number;
}

export interface WriteResult {
  relPath: string;
  newMtime: number;
}

export const ping = (): Promise<string> => call<string>('ping');

export const vaultList = (): Promise<VaultSummary[]> => call<VaultSummary[]>('vault_list');

export const noteRead = (relPath: string): Promise<string> => call<string>('note_read', { args: { relPath } });

export const noteWrite = (relPath: string, content: string, baseMtime?: number): Promise<WriteResult> =>
  call<WriteResult>('note_write', { args: { relPath, content, baseMtime } });

export const vaultClose = (): Promise<void> => callVoid('vault_close');

export interface VaultInfo {
  root: string;
  caseInsensitiveFs: boolean;
}

export interface IndexStatus {
  indexDir: string;
  indexDb: string;
  ready: boolean;
  /** 索引库 schema 版本（未就绪时为 undefined） */
  schemaVersion?: number;
  /** 索引签名摘要前 12 位（诊断用） */
  signaturePrefix?: string;
  /** 上次建库时间戳（毫秒） */
  builtAt?: number;
}

export const vaultOpen = (absPath: string): Promise<VaultInfo> => call<VaultInfo>('vault_open', { args: { absPath } });

export const indexStatus = (): Promise<IndexStatus> => call<IndexStatus>('index_status');

export interface SystemInfo {
  platform: string;
  arch: string;
  version: string;
}

export const systemInfo = (): Promise<SystemInfo> => call<SystemInfo>('system_info');
