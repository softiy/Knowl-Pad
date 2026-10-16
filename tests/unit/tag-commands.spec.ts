import { beforeEach, describe, expect, it, vi } from 'vitest';

const hoisted = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('@core/ipc/client', () => ({ call: hoisted.call, callVoid: vi.fn() }));

import { tagNotes, tagTree } from '@core/ipc/tagCommands';

/**
 * 标签域 IPC 绑定（PRD §5.3.6）：**命令名必须与 PRD 命令表逐字一致** ——
 * 后端 `main.rs` 的 invoke_handler 注册的是 `tag_tree` / `tag_notes`，
 * 前端写错一个字母就会在真机上"命令不存在"（而单测不 mock 客户端时是看不出来的）。
 */
describe('标签域 IPC 绑定', () => {
  beforeEach(() => hoisted.call.mockReset());

  it('tag_tree：命令名逐字一致、无入参', async () => {
    hoisted.call.mockResolvedValue([]);
    await tagTree();
    expect(hoisted.call).toHaveBeenCalledWith('tag_tree');
  });

  it('tag_notes：命令名与入参键名与契约一致（camelCase）', async () => {
    hoisted.call.mockResolvedValue([]);
    await tagNotes('项目/alpha', true);
    expect(hoisted.call).toHaveBeenCalledWith('tag_notes', {
      args: { tagNorm: '项目/alpha', includeChildren: true },
    });
  });

  it('tag_notes：includeChildren 缺省 false（FR-TAG-03 的"含子级"要显式开）', async () => {
    hoisted.call.mockResolvedValue([]);
    await tagNotes('项目');
    expect(hoisted.call).toHaveBeenCalledWith('tag_notes', {
      args: { tagNorm: '项目', includeChildren: false },
    });
  });

  it('未分类查询：空串照样透传（后端据此走"无标签笔记"分支）', async () => {
    hoisted.call.mockResolvedValue([]);
    await tagNotes('');
    expect(hoisted.call).toHaveBeenCalledWith('tag_notes', {
      args: { tagNorm: '', includeChildren: false },
    });
  });
});
