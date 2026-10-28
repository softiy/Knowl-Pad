/**
 * 命令面板的**本地模糊匹配**（FR-PALETTE-02 / FR-PALETTE-10）。
 *
 * 为什么 fuse.js：PRD 明确要求（7.5.0），且命令集合小、匹配在本地完成 —— 不走 IPC（FR-PALETTE-10 的口径）。
 * 这里只做「纯函数 + 预编译索引」，UI 与 store 都不关心 fuse 的细节，测试可以直接钉性能与排序。
 */
import Fuse, { type IFuseOptions } from 'fuse.js';
import type { PaletteCommand } from './registry';

/** 空查询时默认展示的条数（FR-PALETTE-04 的 N 取 12；可展开是 P1，未做）。 */
export const PAGE_SIZE = 12;

const OPTIONS: IFuseOptions<PaletteCommand> = {
  // 名称权重高于关键词：拼音缩写只用于命中，不喧宾夺主
  keys: [
    { name: 'name', weight: 0.7 },
    { name: 'keywords', weight: 0.3 },
  ],
  threshold: 0.4,
  ignoreLocation: true,
  includeScore: true,
  minMatchCharLength: 1,
};

/** 新建索引（命令集合固定，索引可缓存；store 里只建一次）。 */
export function createCommandIndex(commands: readonly PaletteCommand[]): Fuse<PaletteCommand> {
  return new Fuse(commands, OPTIONS);
}

/**
 * 模糊匹配：空查询返回前 `limit` 条（默认顺序）；否则按 fuse 分数返回前 `limit` 条。
 *
 * 纯函数 —— 每次调用新建索引只用于测试/冷路径；热路径（store）用 `createCommandIndex` 缓存。
 */
export function searchCommands(
  query: string,
  commands: readonly PaletteCommand[],
  limit: number = PAGE_SIZE,
): PaletteCommand[] {
  const trimmed = query.trim();
  if (trimmed === '') return commands.slice(0, limit);
  const fuse = new Fuse(commands, OPTIONS);
  return fuse
    .search(trimmed)
    .slice(0, limit)
    .map((hit) => hit.item);
}
