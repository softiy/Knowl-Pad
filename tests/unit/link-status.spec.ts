import { describe, expect, it } from 'vitest';
import {
  applyLinkStatus,
  buildLinkStatusIndex,
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
