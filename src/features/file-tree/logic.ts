/**
 * 文件树的**纯逻辑**（排序 / Wikilink 文本）：与 store 分开，便于单测直接钉口径
 * （FR-FILE-02 排序、FR-FILE-07「复制 Wikilink」，DEBT-19）。
 */
import type { FileNode } from '@core/ipc/commands';
import type { TreeSortMode } from './types';

/** 扩展名（小写、不含点；无扩展名/隐藏文件返回空串）。 */
export function extensionOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i <= 0 ? '' : name.slice(i + 1).toLowerCase();
}

/**
 * FR-FILE-02 的排序口径（PRD 原文：「按名称排序（文件夹优先）；提供按修改时间、按类型的排序选项」）：
 * - **文件夹永远优先**，三种模式都如此（这是原文的硬性要求，不是默认值的副产品）；
 * - `name`：按名称（`zh-CN` + `numeric`，`a2` 排在 `a10` 前）；
 * - `mtime`：**新的在前**（列表用途里"最近改过的先看到"最常用；PRD 未规定方向，此处定死在注释里）；
 * - `type`：按扩展名分组（`md`/`png`/`pdf`…），同扩展名再按名称；无扩展名排在同组末尾（`''` 自然最小？——
 *   这里显式把它排最后：空串按字典序会跑到最前，与直觉相反）。
 *
 * 返回**新数组**：调用方（store 的 cache）拿到的必须是可安全替换的新列表。
 */
export function sortNodes(nodes: readonly FileNode[], mode: TreeSortMode): FileNode[] {
  const byName = (a: FileNode, b: FileNode): number =>
    a.name.localeCompare(b.name, 'zh-CN', { numeric: true, sensitivity: 'base' });
  const byExtension = (a: FileNode, b: FileNode): number => {
    const ea = extensionOf(a.name);
    const eb = extensionOf(b.name);
    if (ea === eb) return 0;
    if (ea === '') return 1;
    if (eb === '') return -1;
    return ea < eb ? -1 : 1;
  };
  const compare = (a: FileNode, b: FileNode): number => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    if (mode === 'mtime') {
      const diff = (b.mtimeMs ?? 0) - (a.mtimeMs ?? 0);
      if (diff !== 0) return diff;
    }
    if (mode === 'type') {
      const diff = byExtension(a, b);
      if (diff !== 0) return diff;
    }
    return byName(a, b);
  };
  return [...nodes].sort(compare);
}

/**
 * FR-FILE-07「复制 Wikilink」的文本：`a/b.md` → `[[a/b]]`（去掉 `.md`，路径分隔符保留 ——
 * Wikilink 的目标本来就是相对路径形态，见 PRD MD-WL-01 与 Rust 侧的 `wikilink.rs`）。
 *
 * 非笔记（附件）不去扩展名：`img/a.png` → `[[img/a.png]]`（`![[...]]` 的嵌入语法用的是同一个目标）。
 */
export function wikilinkOf(relPath: string): string {
  const target = relPath.toLowerCase().endsWith('.md') ? relPath.slice(0, -3) : relPath;
  return '[[' + target + ']]';
}