/**
 * 删除前的引用计数（**FR-FILE-32** / **AC-FILE-11** / DEBT-29）：删笔记前告诉用户"它被多少处引用"。
 *
 * **复用既有反链查询**（`link_backlinks`，与反链面板同一条命令）：DEBT-29 的处置口径就是
 * "删除前用**同一查询**给出 N 处引用提示"—— 本模块不新增命令，也不新造查询。
 *
 * 口径（与 AC-FILE-11 对齐）：
 * - N 是**引用处数**，不是来源文件数（同一篇里写两次 `[[A]]` 算 2 处）；
 * - **嵌入也算引用**（`![[A]]` 删掉后同样变成悬空嵌入）⇒ `includeEmbeds = true`；
 * - 只统计**笔记**（`.md`）：FR-FILE-32 的原文是"删除被引用笔记时"，附件不制造噪声（`count = null`）。
 *
 * 失败处理（R-15，不静默）：查询抛错时返回 `error` 文案、`count = null`，由界面如实提示
 * "无法统计引用，删除前请自行确认" —— **不用 0 冒充失败**（0 是"确实没有引用"，两者语义不同）。
 */
import { asKpError } from '@core/ipc/errors';
import { linkBacklinks } from '@core/ipc/linkCommands';

export interface ReferenceSummary {
  /** 引用处数；`null` 表示"不统计 / 未取到"（**不是** 0 —— 0 表示确实没有引用）。 */
  count: number | null;
  /** 查询失败的可读原因；成功为 null。 */
  error: string | null;
}

/** 统计某笔记被多少处引用（见模块头的口径）。 */
export async function countReferences(relPath: string): Promise<ReferenceSummary> {
  if (!relPath.toLowerCase().endsWith('.md')) return { count: null, error: null };
  try {
    const groups = await linkBacklinks(relPath, true);
    const count = groups.reduce((sum, g) => sum + g.linkCount + g.embedCount, 0);
    return { count, error: null };
  } catch (err) {
    return { count: null, error: asKpError(err).message };
  }
}