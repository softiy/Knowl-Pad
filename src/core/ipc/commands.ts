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

export const noteRead = (relPath: string): Promise<NoteContent> => call<NoteContent>('note_read', { args: { relPath } });

/** 文件树（**单层**）：parentRelPath 缺省为 Vault 根 */
export const fileTree = (parentRelPath?: string, includeHidden?: boolean): Promise<FileNode[]> =>
  call<FileNode[]>('file_tree', { args: { parentRelPath, includeHidden } });

/** 单层目录列表（懒加载；默认不显示隐藏项） */
export const fileListDir = (relPath: string): Promise<FileNode[]> =>
  call<FileNode[]>('file_list_dir', { args: { relPath } });

/** 文件/目录元信息 */
export const fileStat = (relPath: string): Promise<FileStat> =>
  call<FileStat>('file_stat', { args: { relPath } });

/** 文件名合法性校验（FR-FILE-12） */
export const fileValidateName = (name: string): Promise<ValidationResult> =>
  call<ValidationResult>('file_validate_name', { args: { name } });

export const noteWrite = (relPath: string, content: string, baseMtime?: number): Promise<WriteResult> =>
  call<WriteResult>('note_write', { args: { relPath, content, baseMtime } });

/** 新建笔记（FR-FILE-10/13）；onConflict 缺省为 cancel（最安全，R-07 不静默覆盖） */
export const noteCreate = (
  relPath: string,
  content = '',
  onConflict?: ConflictPolicy,
): Promise<WriteResult> => call<WriteResult>('note_create', { args: { relPath, content, onConflict } });

/** 新建文件夹（FR-FILE-11：支持 a/b/c 多级一次创建） */
export const folderCreate = (relPath: string): Promise<void> =>
  callVoid('folder_create', { args: { relPath } });

/** 重命名（**不含链接改写**，改写属 M4） */
export const fileRename = (
  from: string,
  to: string,
  onConflict?: ConflictPolicy,
): Promise<RenameResult> => call<RenameResult>('file_rename', { args: { from, to, onConflict } });

/** 移动（与重命名同一实现，跨目录即移动） */
export const fileMove = (
  from: string,
  to: string,
  onConflict?: ConflictPolicy,
): Promise<RenameResult> => call<RenameResult>('file_move', { args: { from, to, onConflict } });

/** 删除 = 软删除（移入回收站，FR-FILE-30；完整回收站流程属 M7） */
export const fileDelete = (relPath: string, recursive = false): Promise<DeleteResult> =>
  call<DeleteResult>('file_delete', { args: { relPath, recursive } });

/** 在系统文件管理器中显示（opener 插件仅 Rust 侧调用） */
export const fileReveal = (relPath: string): Promise<void> =>
  callVoid('file_reveal', { args: { relPath } });

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

/** 置顶 / 取消置顶（FR-VAULT-07；PRD 勘误 D-10 补齐的命令契约） */
export const vaultPin = (vaultId: number, pinned: boolean): Promise<void> =>
  callVoid('vault_pin', { args: { vaultId, pinned } });

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

// ── 文件域契约（M2 目标形状；实现随 PR-2/PR-3 落地）────────────────────────
// 权威定义见 PRD §5.3.2.1；此处仅冻结 TS 侧形状，**不提供未实现的 invoke 封装**。

/** 文件树节点（**单层**返回，作为虚拟滚动数据源） */
export interface FileNode {
  /** Vault 内相对路径，统一 / 分隔 */
  relPath: string;
  name: string;
  isDir: boolean;
  kind: "note" | "attachment" | "other";
  sizeBytes?: number;
  mtimeMs?: number;
  /** 目录：是否含可见子项（决定展开箭头是否显示） */
  hasChildren?: boolean;
}

/** 笔记内容（编辑器加载 + 冲突检测基线） */
export interface NoteContent {
  relPath: string;
  content: string;
  /** 作为 note_write 的 baseMtime（FR-EDITOR-34 冲突检测） */
  mtimeMs: number;
  sizeBytes: number;
}

/** 文件元信息 */
export interface FileStat {
  relPath: string;
  isDir: boolean;
  kind: "note" | "attachment" | "other";
  sizeBytes: number;
  mtimeMs: number;
}

/** 文件名合法性（FR-FILE-12；reason 为中文可操作提示，ERR-02） */
export interface ValidationResult {
  valid: boolean;
  reason?: string;
}

/** 重命名 / 移动结果（不含链接改写，改写属 M4） */
export interface RenameResult {
  from: string;
  to: string;
  newMtimeMs: number;
}

/** 删除结果（软删除 → 回收站；FR-FILE-30） */
export interface DeleteResult {
  relPath: string;
  /** 实际移入回收站的条目数（目录递归时 > 1） */
  trashedCount: number;
}

/** 重名冲突处理策略（FR-FILE-13：覆盖 / 重命名新建 / 取消） */
export type ConflictPolicy = "overwrite" | "renameNew" | "cancel";
