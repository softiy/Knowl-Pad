import { call, callVoid } from './client';

export interface VaultSummary {
  id: number;
  absPath: string;
  displayName: string;
  /** 最近打开时间（毫秒时间戳） */
  lastOpened?: number;
  /** 是否置顶 */
  pinned?: boolean;
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
  /** 显示名（默认取目录名，可被用户重命名） */
  displayName: string;
  /** 全局库中的注册 id（全局库不可用时为 undefined） */
  vaultId?: number;
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

/** 新建 Vault（目录不存在时创建，并初始化 .knowlpad/ 与空索引库） */
export const vaultCreate = (absPath: string, name?: string): Promise<VaultInfo> =>
  call<VaultInfo>('vault_create', { args: { absPath, name } });

/** 当前 Vault（未打开时为 null） */
export const vaultCurrent = (): Promise<VaultInfo | null> => call<VaultInfo | null>('vault_current');

/** 从列表移除（仅删注册记录，不删磁盘文件） */
export const vaultRegisterRemove = (vaultId: number): Promise<void> =>
  callVoid('vault_register_remove', { args: { vaultId } });

/** 修改显示名 */
export const vaultRename = (vaultId: number, displayName: string): Promise<void> =>
  callVoid('vault_rename', { args: { vaultId, displayName } });

/** 路径失效后重新定位 */
export const vaultRelocate = (vaultId: number, newAbsPath: string): Promise<VaultInfo> =>
  call<VaultInfo>('vault_relocate', { args: { vaultId, newAbsPath } });

export interface SystemInfo {
  platform: string;
  arch: string;
  version: string;
}

export const systemInfo = (): Promise<SystemInfo> => call<SystemInfo>('system_info');
