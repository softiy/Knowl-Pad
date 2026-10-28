import { describe, expect, it } from 'vitest';
import { clickIntent, hasJumpModifier, suggestNoteName } from '@features/editor/logic/linkRouting';

/** 阅读态点击路由（FR-EDITOR-25 / FR-EDITOR-27）：三条口径各一组用例。 */
describe('点击路由：修饰键判定（NFR-PLAT-07）', () => {
  it('Ctrl 与 Cmd 都算跳转修饰键', () => {
    expect(hasJumpModifier({ ctrlKey: true })).toBe(true);
    expect(hasJumpModifier({ metaKey: true })).toBe(true);
    expect(hasJumpModifier({})).toBe(false);
    expect(hasJumpModifier({ ctrlKey: false, metaKey: false })).toBe(false);
  });
});

describe('点击路由：建议笔记名（FR-EDITOR-27 的创建流程）', () => {
  it('缺后缀时补 .md（本仓新建笔记一律带扩展名）', () => {
    expect(suggestNoteName('缺笔记')).toBe('缺笔记.md');
  });

  it('已有 .md 不重复补（大小写不敏感）', () => {
    expect(suggestNoteName('缺笔记.MD')).toBe('缺笔记.MD');
    expect(suggestNoteName('缺笔记.md')).toBe('缺笔记.md');
  });

  it('只取最后一段：[[notes/缺笔记]] ⇒ 缺笔记.md（不凭空建目录）', () => {
    expect(suggestNoteName('notes/缺笔记')).toBe('缺笔记.md');
    expect(suggestNoteName('a/b/c/缺笔记.md')).toBe('缺笔记.md');
  });

  it('空目标 / 只有斜杠 ⇒ 空串（调用方据此不弹对话框）', () => {
    expect(suggestNoteName('')).toBe('');
    expect(suggestNoteName('///')).toBe('');
  });
});

describe('点击路由：意图判定（FR-EDITOR-25 / FR-EDITOR-27）', () => {
  it('Ctrl/Cmd+点击已裁决链接 ⇒ 跳转', () => {
    expect(clickIntent({ target: '笔记A', dangling: false }, { ctrlKey: true })).toEqual({ kind: 'link', target: '笔记A' });
  });

  it('普通点击已裁决链接 ⇒ 不动作（阅读时不该点一下就跳走）', () => {
    expect(clickIntent({ target: '笔记A', dangling: false }, {})).toEqual({ kind: 'none' });
  });

  it('普通点击悬空链接 ⇒ 创建，建议名带 .md', () => {
    expect(clickIntent({ target: '缺笔记', dangling: true }, {})).toEqual({ kind: 'create', name: '缺笔记.md' });
  });

  it('Ctrl+点击悬空链接 ⇒ 仍按"跳转"处理（用户显式要求跳，交给上层报错/提示）', () => {
    expect(clickIntent({ target: '缺笔记', dangling: true }, { metaKey: true })).toEqual({ kind: 'link', target: '缺笔记' });
  });

  it('空目标一律不动作（DOM 属性可能被清空）', () => {
    expect(clickIntent({ target: '', dangling: true }, { ctrlKey: true })).toEqual({ kind: 'none' });
  });
});

  it('FR-EDITOR-25：Ctrl/Cmd+Alt+点击 ⇒ 新标签页意图（与普通跳转区分）', () => {
    expect(clickIntent({ target: 'x', dangling: false }, { ctrlKey: true, altKey: true })).toEqual({ kind: 'link-new-tab', target: 'x' });
    expect(clickIntent({ target: 'x', dangling: false }, { metaKey: true, altKey: true })).toEqual({ kind: 'link-new-tab', target: 'x' });
    expect(clickIntent({ target: 'x', dangling: false }, { ctrlKey: true })).toEqual({ kind: 'link', target: 'x' });
  });
