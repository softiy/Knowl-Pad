import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import { MarkdownView } from '@features/editor';

describe('MarkdownView（阅读态）', () => {
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

  it('点击远程图片占位符向外抛 remoteImage（容器级事件委托）', async () => {
    const wrapper = mount(MarkdownView, { props: { source: '![x](https://evil.example/a.png)' } });
    const placeholder = wrapper.find('[data-kp-remote-src]');
    expect(placeholder.exists()).toBe(true);
    await placeholder.trigger('click');
    expect(wrapper.emitted('remoteImage')?.[0]).toEqual(['https://evil.example/a.png']);
  });

  it('回车键同样触发占位符动作（键盘可达）', async () => {
    const wrapper = mount(MarkdownView, { props: { source: '![x](https://evil.example/a.png)' } });
    await wrapper.find('[data-kp-remote-src]').trigger('keydown', { key: 'Enter' });
    expect(wrapper.emitted('remoteImage')).toHaveLength(1);
  });

  it('点击 data-kp-link 元素抛出内部链接事件', async () => {
    const wrapper = mount(MarkdownView, {
      props: { source: '<span data-kp-link="notes/a.md">A</span>' },
    });
    await wrapper.find('[data-kp-link]').trigger('click');
    expect(wrapper.emitted('link')?.[0]).toEqual(['notes/a.md']);
  });

  it('resolveAsset 生效（本地附件）', () => {
    const wrapper = mount(MarkdownView, {
      props: { source: '![图](a.png)', resolveAsset: (src: string) => `asset://v/${src}` },
    });
    expect(wrapper.html()).toContain('src="asset://v/a.png"');
    expect(wrapper.html()).toContain('referrerpolicy="no-referrer"');
  });
});
