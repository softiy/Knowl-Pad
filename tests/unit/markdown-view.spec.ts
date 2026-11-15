import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * MarkdownView（阅读态）：扩展语法的渲染落地 + **点击语义**（FR-EDITOR-25/27、FR-TAG-05）。
 *
 * 链接状态来自 `link_outgoing`（索引阶段的裁决），因此这里把它整体替身：
 * "什么算悬空"由索引决定，本组件只负责**打标 + 按标分发点击**。
 */
const hoisted = vi.hoisted(() => ({ outgoing: vi.fn() }));
vi.mock('@core/ipc/linkCommands', () => ({ linkOutgoing: hoisted.outgoing }));

/** 远程图片占位符的"打开"必须走系统浏览器（E2 / SEC-08）：替身掉 IPC 封装，断言调用。 */
const external = vi.hoisted(() => ({ openExternal: vi.fn() }));
vi.mock('@core/ipc/externalCommands', () => ({ openExternal: external.openExternal }));

import { MarkdownView } from '@features/editor';
import type { OutgoingLink } from '@core/ipc/linkCommands';

beforeEach(() => {
  external.openExternal.mockReset().mockResolvedValue(undefined);
});

function link(over: Partial<OutgoingLink> & { targetRef: string; status: string }): OutgoingLink {
  return { linkKind: 'wikilink', line: 1, col: 1, dstRelPath: null, ...over };
}

async function mountWith(source: string, relPath = 'a.md') {
  const wrapper = mount(MarkdownView, { props: { source, relPath } });
  await flushPromises();
  return wrapper;
}

describe('MarkdownView（阅读态）：渲染扩展语法', () => {
  beforeEach(() => hoisted.outgoing.mockReset().mockResolvedValue([]));

  it('渲染净化后的 HTML', () => {
    const wrapper = mount(MarkdownView, { props: { source: '# 标题\n\n正文' } });
    expect(wrapper.html()).toContain('<h1>标题</h1>');
    expect(wrapper.find('[data-kp-sanitized="true"]').exists()).toBe(true);
  });

  it('危险内容不进入 DOM（AC-EDITOR-01）', () => {
    const wrapper = mount(MarkdownView, {
      props: { source: '<script>alert(1)</script><img src=x onerror=alert(2)>' },
    });
    expect(wrapper.html()).not.toMatch(/script/i);
    expect(wrapper.html()).not.toMatch(/onerror/i);
  });

  it('wikilink 渲染成可点击节点（data-kp-link 保留到 DOM）', async () => {
    const wrapper = await mountWith('见 [[笔记A]]');
    const el = wrapper.find('[data-kp-link]');
    expect(el.exists()).toBe(true);
    expect(el.attributes('data-kp-link')).toBe('笔记A');
  });

  it('标签渲染成可点击节点（data-kp-tag 保留到 DOM）', async () => {
    const wrapper = await mountWith('这是 #项目/alpha');
    expect(wrapper.find('[data-kp-tag]').attributes('data-kp-tag')).toBe('项目/alpha');
  });

  it('点击远程图片占位符 ⇒ 交给系统浏览器打开（E2：应用内不加载，SEC-08）', async () => {
    const wrapper = mount(MarkdownView, { props: { source: '![x](https://evil.example/a.png)' } });
    const placeholder = wrapper.find('[data-kp-remote-src]');
    expect(placeholder.exists()).toBe(true);
    await placeholder.trigger('click');
    // 打开动作直接走 core/ipc（组件内），emit 保留供上层/测试观测
    expect(external.openExternal).toHaveBeenCalledWith('https://evil.example/a.png');
    expect(wrapper.emitted('remoteImage')?.[0]).toEqual(['https://evil.example/a.png']);
    expect(wrapper.html()).not.toContain('<img');
  });

  it('openExternal 失败 ⇒ console.warn 可见降级，不产生未处理拒绝（R-15）', async () => {
    external.openExternal.mockRejectedValueOnce({ code: 'E_PATH_ESCAPE_DENY', message: '不允许的链接' });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const wrapper = mount(MarkdownView, { props: { source: '![x](https://evil.example/a.png)' } });
      await wrapper.find('[data-kp-remote-src]').trigger('click');
      await flushPromises();
      expect(warn).toHaveBeenCalledTimes(1);
      expect(String(warn.mock.calls[0]?.[0])).toContain('不允许的链接');
    } finally {
      warn.mockRestore();
    }
  });

  it('回车键同样触发占位符动作（键盘可达）', async () => {
    const wrapper = mount(MarkdownView, { props: { source: '![x](https://evil.example/a.png)' } });
    await wrapper.find('[data-kp-remote-src]').trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('remoteImage')).toHaveLength(1);
    expect(external.openExternal).toHaveBeenCalledWith('https://evil.example/a.png');
  });

  it('resolveAsset 生效（本地附件）', () => {
    const wrapper = mount(MarkdownView, {
      props: { source: '![图](a.png)', resolveAsset: (src: string) => `asset://v/${src}` },
    });
    expect(wrapper.html()).toContain('src="asset://v/a.png"');
    expect(wrapper.html()).toContain('referrerpolicy="no-referrer"');
  });
});

describe('MarkdownView：链接状态与点击语义（FR-EDITOR-25 / FR-EDITOR-27）', () => {
  beforeEach(() => hoisted.outgoing.mockReset().mockResolvedValue([]));

  it('悬空链接被打上 kp-link--dangling（视觉区分）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '缺笔记', status: 'dangling' })]);
    const wrapper = await mountWith('见 [[缺笔记]]');
    expect(hoisted.outgoing).toHaveBeenCalledWith('a.md');
    expect(wrapper.find('[data-kp-link]').classes()).toContain('kp-link--dangling');
  });

  it('已裁决的链接**不带**悬空类；同页两个链接各自判定', async () => {
    hoisted.outgoing.mockResolvedValue([
      link({ targetRef: '有的', status: 'resolved', dstRelPath: '有的.md' }),
      link({ targetRef: '没有的', status: 'dangling' }),
    ]);
    const wrapper = await mountWith('[[有的]] 与 [[没有的]]');
    const els = wrapper.findAll('[data-kp-link]');
    expect(els[0]?.classes()).not.toContain('kp-link--dangling');
    expect(els[1]?.classes()).toContain('kp-link--dangling');
  });

  it('没有 relPath 时不查索引（链接保持中性样式，不冒充悬空）', async () => {
    const wrapper = mount(MarkdownView, { props: { source: '[[x]]' } });
    await flushPromises();
    expect(hoisted.outgoing).not.toHaveBeenCalled();
    expect(wrapper.find('[data-kp-link]').classes()).not.toContain('kp-link--dangling');
  });

  it('Ctrl+点击已裁决链接 ⇒ emit link，且 target 换成真实 relPath（D2/10.7）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '有的', status: 'resolved', dstRelPath: 'notes/有的.md' })]);
    const wrapper = await mountWith('[[有的]]');
    await wrapper.find('[data-kp-link]').trigger('click', { ctrlKey: true });
    // 事件是**两段载荷**：目标/形态/悬空态 + 原始鼠标事件（要读修饰键）—— 第二条只用 toBeInstanceOf 粗验
    const [payload, mouseEvent] = wrapper.emitted('link')?.[0] ?? [];
    // 显示目标是 '有的'，但上层（note_read/openNoteInNewTab）只认识 relPath：必须换掉载荷
    expect(payload).toEqual({ target: 'notes/有的.md', kind: 'link', dangling: false });
    expect(mouseEvent).toBeInstanceOf(MouseEvent);
  });

  it('带别名的 wikilink：显示别名不变，点击仍用目标 relPath（[[B|别名]]）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: 'B', status: 'resolved', dstRelPath: 'dir/B.md' })]);
    const wrapper = await mountWith('见 [[B|蓝色别名]]');
    const el = wrapper.find('[data-kp-link]');
    expect(el.text()).toBe('蓝色别名');
    expect(el.attributes('data-kp-link')).toBe('B');
    await el.trigger('click', { ctrlKey: true });
    expect(wrapper.emitted('link')?.[0]?.[0]).toEqual({ target: 'dir/B.md', kind: 'link', dangling: false });
  });

  it('Ctrl/Cmd+Alt+点击 ⇒ 同一 payload（新标签分支由布局层读事件判定），target 已是 relPath', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '有的', status: 'resolved', dstRelPath: 'notes/有的.md' })]);
    const wrapper = await mountWith('[[有的]]');
    await wrapper.find('[data-kp-link]').trigger('click', { ctrlKey: true, altKey: true });
    const [payload, mouseEvent] = wrapper.emitted('link')?.[0] ?? [];
    expect(payload).toEqual({ target: 'notes/有的.md', kind: 'link', dangling: false });
    expect(mouseEvent).toBeInstanceOf(MouseEvent);
    expect((mouseEvent as MouseEvent).altKey).toBe(true);
  });

  it('Ctrl+点击**歧义**链接仍发原显示目标（不猜路径）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '重名', status: 'ambiguous', dstRelPath: 'a/重名.md' })]);
    const wrapper = await mountWith('[[重名]]');
    await wrapper.find('[data-kp-link]').trigger('click', { ctrlKey: true });
    expect(wrapper.emitted('link')?.[0]?.[0]).toEqual({ target: '重名', kind: 'link', dangling: false });
  });

  it('普通点击已裁决链接 ⇒ **不动作**（避免阅读时误跳）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '有的', status: 'resolved', dstRelPath: '有的.md' })]);
    const wrapper = await mountWith('[[有的]]');
    await wrapper.find('[data-kp-link]').trigger('click');
    expect(wrapper.emitted('link')).toBeUndefined();
  });

  it('普通点击**悬空**链接 ⇒ emit link 且 dangling=true（交给上层走创建，FR-EDITOR-27）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '缺笔记', status: 'dangling' })]);
    const wrapper = await mountWith('[[缺笔记]]');
    await wrapper.find('[data-kp-link]').trigger('click');
    const [payload] = wrapper.emitted('link')?.[0] ?? [];
    expect(payload).toEqual({ target: '缺笔记', kind: 'link', dangling: true });
  });

  it('点击标签 ⇒ emit tag（FR-TAG-05；不带修饰键也响应，标签不是"跳转"）', async () => {
    const wrapper = await mountWith('这是 #项目/alpha');
    await wrapper.find('[data-kp-tag]').trigger('click');
    expect(wrapper.emitted('tag')?.[0]).toEqual(['项目/alpha']);
  });

  it('嵌入 `![[x]]` 的点击与普通链接同路（kind=embed）', async () => {
    hoisted.outgoing.mockResolvedValue([link({ targetRef: '图片.png', status: 'dangling', linkKind: 'embed' })]);
    const wrapper = await mountWith('![[图片.png]]');
    await wrapper.find('[data-kp-link]').trigger('click');
    const [payload] = wrapper.emitted('link')?.[0] ?? [];
    expect(payload).toEqual({ target: '图片.png', kind: 'embed', dangling: true });
  });
});
