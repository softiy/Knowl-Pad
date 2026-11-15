/**
 * 文件树的**纯逻辑**（排序 / 路径归一化 / Wikilink 文本）：与 store 分开，便于单测直接钉口径
 * （FR-FILE-02 排序、FR-FILE-07「复制 Wikilink」，DEBT-19；M5 冒烟 5.3/5.4 的回归口径）。
 */
import type { FileNode } from '@core/ipc/commands';
import type { SortDirection, TreeSortMode } from './types';

/** 扩展名（小写、不含点；无扩展名/隐藏文件返回空串）。 */
export function extensionOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i <= 0 ? '' : name.slice(i + 1).toLowerCase();
}

/**
 * 名称的自然序比较（zh-CN + numeric：a2 在 a10 前）。
 *
 * 最后那一步不是多余的：sensitivity: 'base' 让 A 与 a 比较结果为 0，
 * 若只依赖它，同名词的大小写顺序就取决于**输入顺序**（服务端返回序）而不是一个确定口径。
 * 这里再按原串收敛，保证「同一批节点 → 永远同一个输出」——排序必须是确定性函数。
 */
export function compareNames(a: string, b: string): number {
  const primary = a.localeCompare(b, 'zh-CN', { numeric: true, sensitivity: 'base' });
  if (primary !== 0) return primary;
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * 每种排序模式的默认方向（PRD 未规定方向，此处定死并留痕）：
 * - name：升序（a2 → a10，符合文件管理器直觉）；
 * - mtime：**降序**（最近改过的先看到；冒烟 5.2/5.3 的「时间最近优先」）；
 * - type：升序（按扩展名分组，md → png → pdf…）。
 */
export function defaultDirectionOf(mode: TreeSortMode): SortDirection {
  return mode === 'mtime' ? 'desc' : 'asc';
}

/** 扩展名分组比较：无扩展名显式排最后（空串按字典序会跑到最前，与直觉相反）。 */
function compareExtensions(a: FileNode, b: FileNode): number {
  const ea = extensionOf(a.name);
  const eb = extensionOf(b.name);
  if (ea === eb) return 0;
  if (ea === '') return 1;
  if (eb === '') return -1;
  return ea < eb ? -1 : 1;
}

/**
 * 主键比较。返回的是**升序**语义（mtime 也是"旧→新"），方向统一在 compareNodes 里翻转 ——
 * 这样每种模式只有一处方向逻辑，不会出现"默认 desc 又被翻回 asc"的双重取反。
 */
function comparePrimary(a: FileNode, b: FileNode, mode: TreeSortMode): number {
  if (mode === 'mtime') {
    // 缺 mtime 的（索引路径下的目录节点）按 0 处理，时间升序下排最前、降序（默认）下排最后。
    // 后端 mtimeMs 是**毫秒截断**（Rust as_millis），同一时钟节拍内创建的文件会真的得到
    // 相同的毫秒值（Windows 文件时间实测如此），因此「并列」不是异常，是常态 ——
    // 必须由 compareNodes 的 tie-break 兜底。
    return (a.mtimeMs ?? 0) - (b.mtimeMs ?? 0);
  }
  if (mode === 'type') return compareExtensions(a, b);
  return compareNames(a.name, b.name);
}

/**
 * **全序**比较：任意两个节点都给出确定结果（不依赖输入顺序）。
 * - 文件夹永远优先（三种模式下都如此；方向只翻转**组内主键**，不翻转目录分组）；
 * - 主键并列（mtime 相同 / 扩展名相同）时 → 名称自然序**升序**兜底。
 *
 * 为什么兜底方向固定为升序、不跟随 direction：并列时若也翻转，两个「看起来一样新」的文件
 * 会随方向按钮来回跳，用户无法建立稳定预期；而「并列时按名称」本身是可解释的规则
 * （冒烟 5.3 的 a10/b.txt 首两位互换正是并列导致的，见 tests/unit/file-tree-logic.spec.ts）。
 */
function compareNodes(a: FileNode, b: FileNode, mode: TreeSortMode, direction: SortDirection): number {
  if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
  const primary = comparePrimary(a, b, mode);
  if (primary !== 0) return direction === 'desc' ? -primary : primary;
  return compareNames(a.name, b.name);
}

/** FR-FILE-02：排序入口。direction 缺省取该模式的默认方向（见 defaultDirectionOf）。 */
export function sortNodes(
  nodes: readonly FileNode[],
  mode: TreeSortMode,
  direction: SortDirection = defaultDirectionOf(mode),
): FileNode[] {
  return [...nodes].sort((a, b) => compareNodes(a, b, mode, direction));
}

/**
 * 相对路径归一化（比较 / 发送命令前统一）：去首尾空白、统一分隔符为 /、折叠重复分隔符、
 * 去掉开头的 ./ 与结尾的 /。R-09：前端不假设平台，反斜杠只在输入侧归一化。
 */
export function normalizeRelPath(relPath: string): string {
  return relPath
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/{2,}/g, '/')
    .replace(/^\.\//, '')
    .replace(/\/+$/, '');
}

/**
 * 两条相对路径是否指向**同一个目标**。caseInsensitive 由平台事实决定
 * （Rust platform::case_insensitive_fs 经 vault_current.caseInsensitiveFs 送来，见 fileOps store），
 * 不在前端硬编码平台（R-09）：Windows/macOS 默认不区分大小写，A.md 与 a.md 是同一个文件。
 */
export function isSameRelPath(a: string, b: string, caseInsensitive: boolean): boolean {
  const na = normalizeRelPath(a);
  const nb = normalizeRelPath(b);
  return caseInsensitive ? na.toLowerCase() === nb.toLowerCase() : na === nb;
}

/**
 * 新建笔记的名称归一（M5 冒烟 5.6：新建笔记默认无扩展名）：
 * - **没有扩展名**时补 .md；已有扩展名（含 .md）原样返回 —— 用户写了 .txt 就是 .txt，不擅自改名；
 * - 空串保持空串，让校验给出「名称不能为空」而不是变成 .md；
 * - 文件夹**不补**扩展名（调用方只在 newNote 用）。
 */
export function ensureNoteExtension(name: string): string {
  const trimmed = name.trim();
  if (trimmed === '' || extensionOf(trimmed) !== '') return trimmed;
  return trimmed + '.md';
}

/**
 * FR-FILE-07「复制 Wikilink」的文本：a/b.md → [[a/b]]（去掉 .md，路径分隔符保留 ——
 * Wikilink 的目标本来就是相对路径形态，见 PRD MD-WL-01 与 Rust 侧的 wikilink.rs）。
 *
 * 非笔记（附件）不去扩展名：img/a.png → [[img/a.png]]（![[...]] 的嵌入语法用的是同一个目标）。
 */
export function wikilinkOf(relPath: string): string {
  const target = relPath.toLowerCase().endsWith('.md') ? relPath.slice(0, -3) : relPath;
  return '[[' + target + ']]';
}
