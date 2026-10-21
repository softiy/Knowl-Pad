import { defineStore } from 'pinia';
import { ref } from 'vue';
import { asKpError } from '@core/ipc/errors';
import {
  fileTree,
  preferenceGet,
  vaultStateGet,
  vaultStateSet,
  type FileNode,
} from '@core/ipc/commands';
import { sortNodes } from '../logic';
import {
  STATE_KEY_SHOW_HIDDEN,
  STATE_KEY_TREE_EXPANDED,
  type TreeRow,
  type TreeSortMode,
} from '../types';

const ROOT = '';

/**
 * 文件树 store：**扁平化 + 懒加载**（技术方案 §7.3）。
 *
 * 设计要点：
 * - 只缓存**已加载层级**（`cache` 非响应式），展开时才请求子节点 → 10 万节点不会一次性进内存；
 * - `rows` 是扁平化可视行数组，虚拟滚动直接消费；
 * - 展开状态持久化到 `vault_state`（AC-FILE-08），隐藏文件开关持久化到全局偏好（FR-FILE-05）。
 */
export const useFileTreeStore = defineStore('fileTree', () => {
  const rows = ref<TreeRow[]>([]);
  const expanded = ref<Set<string>>(new Set());
  const includeHidden = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** 已加载层级缓存：父目录相对路径 → 子节点（根为 ''）；**存入前已按当前排序模式排好** */
  const cache = new Map<string, FileNode[]>();
  /** FR-FILE-02：排序模式（默认名称；纯视图状态，不持久化 —— 见 DEBT-19 的处置说明） */
  const sortMode = ref<TreeSortMode>('name');

  /** 依据 `expanded` 与缓存把树压平成一维可视行。 */
  function flatten(): TreeRow[] {
    const out: TreeRow[] = [];
    const walk = (parent: string, depth: number): void => {
      for (const node of cache.get(parent) ?? []) {
        const isExpanded = node.isDir && expanded.value.has(node.relPath);
        out.push({
          relPath: node.relPath,
          name: node.name,
          isDir: node.isDir,
          kind: node.kind,
          depth,
          hasChildren: node.hasChildren ?? false,
          expanded: isExpanded,
        });
        if (isExpanded) {
          walk(node.relPath, depth + 1);
        }
      }
    };
    walk(ROOT, 0);
    return out;
  }

  async function loadChildren(relPath: string): Promise<void> {
    const nodes = await fileTree(relPath === ROOT ? undefined : relPath, includeHidden.value);
    cache.set(relPath, sortNodes(nodes, sortMode.value));
  }

  /**
   * FR-FILE-02：切换排序 —— **纯前端重排，不重新请求**（缓存里的每层按新模式再排一次即可）。
   * 不持久化：排序是"当前视图怎么看"，PRD §4.11.3 的设置项清单里没有它；要持久化应作为设置项另行登记。
   */
  function setSortMode(mode: TreeSortMode): void {
    if (sortMode.value === mode) return;
    sortMode.value = mode;
    for (const [parent, nodes] of [...cache.entries()]) {
      cache.set(parent, sortNodes(nodes, mode));
    }
    rows.value = flatten();
  }

  /** 载入根层并重建可视行。 */
  async function loadRoot(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      cache.clear();
      await loadChildren(ROOT);
      rows.value = flatten();
    } catch (err) {
      error.value = asKpError(err).message;
    } finally {
      loading.value = false;
    }
  }

  /** 逐层展开某路径（其祖先必须已加载）。 */
  async function expandPath(relPath: string): Promise<void> {
    const segments = relPath.split('/');
    let parent = ROOT;
    for (const segment of segments) {
      const current = parent === ROOT ? segment : `${parent}/${segment}`;
      if (!cache.has(current)) {
        await loadChildren(current);
      }
      expanded.value = new Set([...expanded.value, current]);
      parent = current;
    }
  }

  /** 持久化展开状态；失败不静默（R-15）。 */
  async function persistExpanded(): Promise<void> {
    try {
      await vaultStateSet({ [STATE_KEY_TREE_EXPANDED]: [...expanded.value].sort() });
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 展开/折叠目录（懒加载其子层）。 */
  async function toggle(relPath: string): Promise<void> {
    const next = new Set(expanded.value);
    if (next.has(relPath)) {
      next.delete(relPath);
      expanded.value = next;
      rows.value = flatten();
      await persistExpanded();
      return;
    }
    next.add(relPath);
    expanded.value = next;
    try {
      if (!cache.has(relPath)) {
        await loadChildren(relPath);
      }
      rows.value = flatten();
    } catch (err) {
      error.value = asKpError(err).message;
      expanded.value = new Set([...expanded.value].filter((p) => p !== relPath));
    }
    await persistExpanded();
  }

  /** 重新加载（文件操作后刷新树）：清缓存 → 重载根 → 恢复原有展开层级。 */
  async function refresh(): Promise<void> {
    const keep = [...expanded.value];
    cache.clear();
    error.value = null;
    try {
      await loadChildren(ROOT);
      for (const path of keep) {
        await expandPath(path);
      }
      rows.value = flatten();
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /**
   * 切换「显示隐藏文件」（FR-FILE-05）：清缓存 → 重载 → 尽力恢复原有展开层级。
   *
   * 落盘位置是**每 Vault 的 `vault_state`**（PRD §4.11.3 把这一项定为 Vault 设置；FR-SET-04）：
   * 写全局 `preference` 会让切换 Vault 时串设置 —— 那就是技术方案 §13.2 的 **DEBT-15**。
   */
  async function setIncludeHidden(value: boolean): Promise<void> {
    includeHidden.value = value;
    const keep = [...expanded.value];
    cache.clear();
    try {
      await loadChildren(ROOT);
      for (const path of keep) {
        await expandPath(path);
      }
      rows.value = flatten();
      await vaultStateSet({ [STATE_KEY_SHOW_HIDDEN]: value });
    } catch (err) {
      error.value = asKpError(err).message;
    }
  }

  /** 恢复上次的展开状态（AC-FILE-08）与隐藏文件偏好（FR-FILE-05）。 */
  async function restoreState(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      // 优先级：**本 Vault 的 vault_state** 是唯一权威（DEBT-15）。
      const hidden = await vaultStateGet([STATE_KEY_SHOW_HIDDEN]);
      const fromVault = hidden.values[STATE_KEY_SHOW_HIDDEN];
      if (typeof fromVault === 'boolean') {
        includeHidden.value = fromVault;
      } else {
        // 一次性兼容：旧版本把同一个值写在**全局 preference**里（那正是 DEBT-15 的缺陷）。
        // 读到就**写回本 Vault**，此后只认 vault_state —— 既不让老用户的选择无声丢失，
        // 也不再让这个值跟着别的 Vault 串。全局那一行不删（它属于用户数据，删它没有收益）。
        const legacy = await preferenceGet([STATE_KEY_SHOW_HIDDEN]);
        const old = legacy.values[STATE_KEY_SHOW_HIDDEN];
        if (typeof old === 'boolean') {
          includeHidden.value = old;
          await vaultStateSet({ [STATE_KEY_SHOW_HIDDEN]: old });
        }
      }
      cache.clear();
      await loadChildren(ROOT);
      const stored = await vaultStateGet([STATE_KEY_TREE_EXPANDED]);
      const list = stored.values[STATE_KEY_TREE_EXPANDED];
      if (Array.isArray(list)) {
        for (const path of list.filter((p): p is string => typeof p === 'string')) {
          await expandPath(path);
        }
      }
      rows.value = flatten();
    } catch (err) {
      error.value = asKpError(err).message;
    } finally {
      loading.value = false;
    }
  }

  return {
    rows,
    expanded,
    includeHidden,
    sortMode,
    setSortMode,
    loading,
    error,
    loadRoot,
    toggle,
    setIncludeHidden,
    restoreState,
    refresh,
    persistExpanded,
  };
});
