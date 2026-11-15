/**
 * 文件树的**持久化与整树装载**（从 fileTree store 拆出；CODE-11 的文件长度纪律）。
 *
 * 为什么单独一层：这些流程都是「读 vault_state / 全局偏好 → 清缓存 → 逐层重新加载」，
 * 与「怎么排列 / 本地增删节点」是两件事。抽出来后 store 只保留编排，口径集中在这里。
 *
 * 依赖方向：本文件只通过 `TreeIOPorts` 操作 store 的状态，**不 import store**（避免循环依赖）。
 */
import type { Ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import {
  preferenceGet,
  vaultStateGet,
  vaultStateSet,
  type FileNode,
} from '@core/ipc/commands';
import { STATE_KEY_SHOW_HIDDEN, STATE_KEY_TREE_EXPANDED, type TreeRow } from '../types';
import { parentOf } from './treeCache';

/** treeIO 读写 store 状态的最小接口。 */
export interface TreeIOPorts {
  cache: Map<string, FileNode[]>;
  expanded: Ref<Set<string>>;
  includeHidden: Ref<boolean>;
  rows: Ref<TreeRow[]>;
  loading: Ref<boolean>;
  error: Ref<string | null>;
  isStale: (gen: number) => boolean;
  loadChildren: (relPath: string, gen: number) => Promise<void>;
  expandPath: (relPath: string, gen?: number) => Promise<void>;
  flatten: () => TreeRow[];
  persistExpanded: () => Promise<void>;
  /** 当前装载代次（ensureTreeVisible 需要与 store 的装载轮次对齐）。 */
  generation: () => number;
}

const ROOT = '';

/**
 * 读取「显示隐藏文件」的**本 Vault 权威值**，并按 DEBT-15 做一次性旧值迁移。
 * 返回 null 表示两处都没有值（保持默认关，不写任何东西）。
 */
async function readIncludeHidden(): Promise<boolean | null> {
  const hidden = await vaultStateGet([STATE_KEY_SHOW_HIDDEN]);
  const fromVault = hidden.values[STATE_KEY_SHOW_HIDDEN];
  if (typeof fromVault === 'boolean') return fromVault;
  // 一次性兼容：旧版本把同一个值写在**全局 preference**里（那正是 DEBT-15 的缺陷）。
  // 读到就**写回本 Vault**，此后只认 vault_state —— 既不让老用户的选择无声丢失，
  // 也不再让这个值跟着别的 Vault 串。全局那一行不删（它属于用户数据，删它没有收益）。
  const legacy = await preferenceGet([STATE_KEY_SHOW_HIDDEN]);
  const old = legacy.values[STATE_KEY_SHOW_HIDDEN];
  if (typeof old !== 'boolean') return null;
  await vaultStateSet({ [STATE_KEY_SHOW_HIDDEN]: old });
  return old;
}

/** 读取上次的展开层级（AC-FILE-08）；非字符串项一律忽略。 */
async function readExpandedPaths(): Promise<string[]> {
  const stored = await vaultStateGet([STATE_KEY_TREE_EXPANDED]);
  const list = stored.values[STATE_KEY_TREE_EXPANDED];
  return Array.isArray(list) ? list.filter((item): item is string => typeof item === 'string') : [];
}

/** 清空缓存并重载根层；代次变了（切库/reset）返回 false，调用方必须立即收手。 */
async function reloadRoot(ports: TreeIOPorts, gen: number): Promise<boolean> {
  ports.cache.clear();
  await ports.loadChildren(ROOT, gen);
  return !ports.isStale(gen);
}

/** 重新加载并恢复原有展开层级（手动刷新）。 */
export async function refreshKeepingExpanded(ports: TreeIOPorts, gen: number): Promise<void> {
  const keep = [...ports.expanded.value];
  ports.error.value = null;
  try {
    if (!(await reloadRoot(ports, gen))) return;
    for (const path of keep) {
      await ports.expandPath(path, gen);
      if (ports.isStale(gen)) return;
    }
    ports.rows.value = ports.flatten();
  } catch (err) {
    if (!ports.isStale(gen)) ports.error.value = asKpError(err).message;
  }
}

/**
 * 切换「显示隐藏文件」（FR-FILE-05）：重新加载并尽力恢复展开层级，成功后写入**本 Vault**的 vault_state。
 *
 * 落盘位置是每 Vault 的 vault_state（PRD §4.11.3 把这一项定为 Vault 设置；FR-SET-04）：
 * 写全局 preference 会让切换 Vault 时串设置 —— 那就是技术方案 §13.2 的 DEBT-15。
 */
export async function setIncludeHiddenAndReload(
  ports: TreeIOPorts,
  gen: number,
  value: boolean,
): Promise<void> {
  ports.includeHidden.value = value;
  const keep = [...ports.expanded.value];
  try {
    if (!(await reloadRoot(ports, gen))) return;
    for (const path of keep) {
      await ports.expandPath(path, gen);
      if (ports.isStale(gen)) return;
    }
    ports.rows.value = ports.flatten();
    await vaultStateSet({ [STATE_KEY_SHOW_HIDDEN]: value });
  } catch (err) {
    if (!ports.isStale(gen)) ports.error.value = asKpError(err).message;
  }
}

/** 恢复上次的展开状态（AC-FILE-08）与隐藏文件偏好（FR-FILE-05）。 */
export async function restoreTreeState(ports: TreeIOPorts, gen: number): Promise<void> {
  const stale = (): boolean => ports.isStale(gen);
  ports.loading.value = true;
  ports.error.value = null;
  // 以持久化值为**唯一权威**：先清空，避免把上一轮（可能是上一个 Vault）的展开目录并入。
  ports.expanded.value = new Set();
  try {
    const hidden = await readIncludeHidden();
    if (stale()) return;
    if (hidden !== null) ports.includeHidden.value = hidden;
    if (!(await reloadRoot(ports, gen))) return;
    const paths = await readExpandedPaths();
    if (stale()) return;
    for (const path of paths) {
      // 单条失效路径（目录已被改名/删除）不得让整棵树恢复失败：跳过它并留痕（R-15 不静默）。
      try {
        await ports.expandPath(path, gen);
      } catch (err) {
        console.warn('文件树展开状态失效，已跳过该路径', path, asKpError(err).message);
      }
      if (stale()) return;
    }
    ports.rows.value = ports.flatten();
  } catch (err) {
    if (!stale()) ports.error.value = asKpError(err).message;
  } finally {
    if (!stale()) ports.loading.value = false;
  }
}
/**
 * 让某路径的**父目录**被加载并展开（新文件要"出现在树里"的前提）；失败不静默。
 * 只展开父级：文件自身不需要展开。
 */
export async function ensureTreeVisible(ports: TreeIOPorts, relPath: string): Promise<void> {
  const parent = parentOf(relPath);
  const gen = ports.generation();
  try {
    if (parent !== ROOT) {
      await ports.expandPath(parent, gen);
      if (ports.isStale(gen)) return;
    }
    if (!ports.cache.has(parent)) {
      await ports.loadChildren(parent, gen);
      if (ports.isStale(gen)) return;
    }
    ports.rows.value = ports.flatten();
    await ports.persistExpanded();
  } catch (err) {
    if (!ports.isStale(gen)) ports.error.value = asKpError(err).message;
  }
}
