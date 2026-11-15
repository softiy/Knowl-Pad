import { describe, expect, it } from 'vitest';
import {
  applyLinkStatus,
  buildLinkStatusIndex,
  dstRelPathOf,
  loadLinkStatus,
  normalizeTarget,
  stateClass,
  stateOfTarget,
} from '@core/markdown/linkStatus';
import type { OutgoingLink } from '@core/ipc/linkCommands';

function link(over: Partial<OutgoingLink> & { targetRef: string; status: string }): OutgoingLink {
  return { linkKind: 'wikilink', line: 1, col: 1, dstRelPath: null, ...over };
}

describe('链接状态索引（FR-EDITOR-27 的判据）', () => {
  it('已裁决 → resolved；无候选 → dangling；多候选 → ambiguous', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'notes/a.md' }),
      link({ targetRef: 'b', status: 'dangling' }),
      link({ targetRef: 'c', status: 'ambiguous' }),
    ]);
    expect(stateOfTarget(index, 'a')).toBe('resolved');
    expect(stateOfTarget(index, 'b')).toBe('dangling');
    expect(stateOfTarget(index, 'c')).toBe('ambiguous');
  });

  it('"resolved 但没有 dstRelPath"按悬空处理（不指向任何文件就不是可跳转的链接）', () => {
    const index = buildLinkStatusIndex([link({ targetRef: 'a', status: 'resolved', dstRelPath: null })]);
    expect(stateOfTarget(index, 'a')).toBe('dangling');
  });

  it('目标比较折叠大小写、分隔符与 .md 后缀；不在清单里 ⇒ null（中性，不冒充悬空）', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'Notes/A.md', status: 'resolved', dstRelPath: 'notes/a.md' }),
    ]);
    expect(normalizeTarget(' notes\\a.MD ')).toBe('notes/a');
    expect(stateOfTarget(index, 'notes/a')).toBe('resolved');
    expect(stateOfTarget(index, 'notes/a.md')).toBe('resolved');
    expect(stateOfTarget(index, '别的笔记')).toBeNull();
  });

  it('同一目标多条时取**最不确定**的那个（宁可提示不确定，也不猜一个目标跳过去）', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'x/a.md' }),
      link({ targetRef: 'A', status: 'ambiguous' }),
    ]);
    expect(stateOfTarget(index, 'a')).toBe('ambiguous');
  });

  it('状态 → 类名：resolved 没有额外类，其余各有一个', () => {
    expect(stateClass('resolved')).toBe('');
    expect(stateClass(null)).toBe('');
    expect(stateClass('dangling')).toBe('kp-link--dangling');
    expect(stateClass('ambiguous')).toBe('kp-link--ambiguous');
  });

  it('applyLinkStatus 把状态打到达 DOM 上，并且**能退掉**旧状态', () => {
    const doc = new DOMParser().parseFromString(
      '<div><span data-kp-link="a"></span><span data-kp-link="b" class="kp-link--dangling"></span></div>',
      'text/html',
    );
    const root = doc.body;
    applyLinkStatus(root, buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'dangling' }),
      link({ targetRef: 'b', status: 'resolved', dstRelPath: 'b.md' }),
    ]));
    const spans = Array.from(root.querySelectorAll('[data-kp-link]'));
    expect(spans[0]?.classList.contains('kp-link--dangling')).toBe(true);
    // b 已被裁决 ⇒ 旧的悬空类必须退掉（否则"刚创建的笔记"会永远显示成悬空）
    expect(spans[1]?.classList.contains('kp-link--dangling')).toBe(false);
  });

  it('查询失败时返回空索引而不是抛（阅读不该因索引失败整页空白）', async () => {
    // 单测环境里 `__TAURI_INTERNALS__.invoke` 直接 reject（见 tests/unit/setup.ts）
    const index = await loadLinkStatus('a.md');
    expect(stateOfTarget(index, 'a')).toBeNull();
  });
});
/**
 * 点击跳转需要的是**真实 relPath**（M5 真机 D2 / 10.7）。
 *
 * 正文里的目标是显示口径（`[[B]]` / `[[B|别名]]` 的 data-kp-link 都是 `B`），
 * 而 `note_read` 只认识 relPath —— 索引里不带 `dstRelPath`，点击就必然报「文件不存在」。
 */
describe('链接裁决索引：携带真实目标路径（D2 / 10.7）', () => {
  it('resolved 链接的 dstRelPath 可查，且容忍大小写 / 分隔符 / .md 后缀', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'Notes/B', status: 'resolved', dstRelPath: 'notes/中文 B.md' }),
    ]);
    expect(dstRelPathOf(index, 'notes/b')).toBe('notes/中文 B.md');
    expect(dstRelPathOf(index, 'notes\\B.MD')).toBe('notes/中文 B.md');
    // 状态本身不受影响
    expect(stateOfTarget(index, 'notes/b')).toBe('resolved');
  });

  it('悬空 / 歧义 / 未知目标都没有 dstRelPath（不猜路径，避免跳错或跳不存在）', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: '悬空', status: 'dangling' }),
      link({ targetRef: '歧义', status: 'ambiguous', dstRelPath: 'a.md' }),
    ]);
    expect(dstRelPathOf(index, '悬空')).toBeNull();
    expect(dstRelPathOf(index, '歧义')).toBeNull();
    expect(dstRelPathOf(index, '不在清单里')).toBeNull();
  });

  it('同目标多条时，最终状态不确定则丢掉路径（宁可不可跳，也不跳错）', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'x/a.md' }),
      link({ targetRef: 'A', status: 'ambiguous' }),
    ]);
    expect(dstRelPathOf(index, 'a')).toBeNull();
  });

  it('同目标多条 resolved（都带路径）：保留其中一条可跳路径', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'dir/a.md' }),
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'dir/a.md' }),
    ]);
    expect(dstRelPathOf(index, 'a')).toBe('dir/a.md');
  });

  it('同目标里有一条 resolved 但没路径 ⇒ 按悬空合并，整条不可跳（保守口径）', () => {
    const index = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'resolved', dstRelPath: null }),
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'dir/a.md' }),
    ]);
    expect(stateOfTarget(index, 'a')).toBe('dangling');
    expect(dstRelPathOf(index, 'a')).toBeNull();
  });

  it('歧义/悬空在前、resolved 在后：仍保持"不可跳"，不产生带路径的不可跳状态', () => {
    // 顺序敏感：合并逻辑曾经在"后一条 resolved 带路径"时给 ambiguous 状态补上 dstRelPath，
    // 点击就会绕过"歧义不猜目标"的裁决 —— 这里两个顺序都钉住。
    const ambiguousFirst = buildLinkStatusIndex([
      link({ targetRef: 'a', status: 'ambiguous' }),
      link({ targetRef: 'a', status: 'resolved', dstRelPath: 'x/a.md' }),
    ]);
    expect(stateOfTarget(ambiguousFirst, 'a')).toBe('ambiguous');
    expect(dstRelPathOf(ambiguousFirst, 'a')).toBeNull();

    const danglingFirst = buildLinkStatusIndex([
      link({ targetRef: 'b', status: 'dangling' }),
      link({ targetRef: 'b', status: 'resolved', dstRelPath: 'x/b.md' }),
    ]);
    expect(stateOfTarget(danglingFirst, 'b')).toBe('dangling');
    expect(dstRelPathOf(danglingFirst, 'b')).toBeNull();
  });

  it('resolved 但后端没给 dstRelPath ⇒ 按悬空处理且没有可跳路径（既有口径）', () => {
    const index = buildLinkStatusIndex([link({ targetRef: 'a', status: 'resolved', dstRelPath: null })]);
    expect(stateOfTarget(index, 'a')).toBe('dangling');
    expect(dstRelPathOf(index, 'a')).toBeNull();
  });
});
