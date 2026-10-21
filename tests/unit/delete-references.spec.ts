/** FR-FILE-32 / AC-FILE-11 / DEBT-29：删除前的引用处数统计（复用既有反链查询）。 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@core/ipc/linkCommands', () => ({ linkBacklinks: vi.fn() }));

import { linkBacklinks } from '@core/ipc/linkCommands';
import { countReferences } from '@features/file-tree/deleteReferences';

const mBacklinks = vi.mocked(linkBacklinks);

function group(srcRelPath: string, linkCount: number, embedCount: number) {
  // 只给被测代码需要的字段（完整形状见 linkCommands.ts 的 BacklinkGroup）
  return { srcRelPath, srcName: srcRelPath, linkCount, embedCount, items: [] };
}

describe('countReferences：FR-FILE-32 的口径', () => {
  // 用例之间清掉调用记录（否则「不调用该命令」的断言会被上一条用例污染）
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it('N = 引用处数（普通链接 + 嵌入，跨来源求和），不是来源文件数', async () => {
    mBacklinks.mockResolvedValue([group('a.md', 3, 0), group('b.md', 1, 1)]);
    const summary = await countReferences('dir/A.md');
    expect(summary.count).toBe(5);
    expect(summary.error).toBeNull();
    // 复用既有命令，且嵌入必须计入（删掉后 ![[A]] 同样变悬空）
    expect(mBacklinks).toHaveBeenCalledWith('dir/A.md', true);
  });

  it('没有任何来源 ⇒ 0（界面据此不提示）', async () => {
    mBacklinks.mockResolvedValue([]);
    expect((await countReferences('A.md')).count).toBe(0);
  });

  it('非笔记（附件）不统计 ⇒ null（不是 0，避免制造噪声）', async () => {
    const summary = await countReferences('img/a.png');
    expect(summary).toEqual({ count: null, error: null });
    expect(mBacklinks).not.toHaveBeenCalled();
  });

  it('查询失败 ⇒ error 有值、count 为 null（R-15：不用 0 冒充）', async () => {
    mBacklinks.mockRejectedValue({ code: 'E_VAULT_NOT_OPEN', message: '知识库未打开' });
    const summary = await countReferences('A.md');
    expect(summary.count).toBeNull();
    expect(summary.error).toBe('知识库未打开');
  });

  it('大小写扩展名同样识别（.MD）', async () => {
    mBacklinks.mockResolvedValue([group('x.md', 2, 0)]);
    expect((await countReferences('A.MD')).count).toBe(2);
  });
});