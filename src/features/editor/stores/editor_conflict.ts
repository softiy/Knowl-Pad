//! 编辑器「冲突处理」三选项（FR-EDITOR-34）的实现。
//!
//! 单独成文件的两个理由：① editor.ts 已贴近 CODE-11 的硬性上限 350 行；
//! ② 这段逻辑只依赖「查 buffer」与「保存」两个动作，天然适合抽出来单独验证。
//!
//! 审计 F3：三个分支里的 noteRead 都可能抛（文件被外部删除），此前没人接 → 对话框"点了没反应"。
//! 现在统一收敛成 buffer.error（由 EditorView 渲染），并沿用 save 的既有做法。

import { asKpError } from '@core/ipc/errors';
import { noteRead } from '@core/ipc/commands';
import { diffLines } from '@core/utils/diff';
import type { ConflictChoice, EditorBuffer } from './editor';

type Finder = (relPath: string) => EditorBuffer | undefined;

/** 读外部版本；失败写 buffer.error 并返回 null。 */
async function readExternal(find: Finder, relPath: string) {
  try {
    return await noteRead(relPath);
  } catch (err) {
    const b = find(relPath);
    if (b) b.error = asKpError(err).message;
    return null;
  }
}

/** 冲突处理三选项（FR-EDITOR-34）：加载外部 / 保留我的并覆盖 / 查看差异。 */
export async function resolveConflictWith(
  find: Finder,
  save: (relPath: string) => Promise<void>,
  relPath: string,
  choice: ConflictChoice,
): Promise<void> {
  const buffer = find(relPath);
  if (!buffer) return;
  if (choice === 'loadExternal') {
    const note = await readExternal(find, relPath);
    if (!note) return;
    buffer.content = note.content;
    buffer.savedContent = note.content;
    buffer.mtimeMs = note.mtimeMs;
    buffer.conflict = false;
    buffer.error = null;
    buffer.replacedAt += 1; // 显式通知视图回灌内核
    return;
  }
  if (choice === 'keepMine') {
    // 基线刷新为磁盘当前版本，再强制覆盖（用户已明确选择「保留我的」）
    const note = await readExternal(find, relPath);
    if (!note) return;
    buffer.mtimeMs = note.mtimeMs;
    buffer.savedContent = note.content;
    await save(relPath);
    return;
  }
  // 查看差异：拉取外部版本与当前内容做行级比较
  const note = await readExternal(find, relPath);
  if (!note) return;
  buffer.diff = diffLines(note.content, buffer.content).lines;
}
