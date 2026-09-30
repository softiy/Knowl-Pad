import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { describe, expect, it, vi } from 'vitest';
import App from '@/App.vue';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';
import { bootstrap } from '@/app/bootstrap';
import { router } from '@/app/router';

vi.mock('@core/ipc/commands', () => ({ vaultList: vi.fn().mockResolvedValue([]) }));

describe('bootstrap', () => {
  it('净化器自检通过（失败会抛错阻断启动）', () => {
    expect(() => {
      bootstrap();
    }).not.toThrow();
  });
});

describe('KpSafeHtml', () => {
  it('渲染净化后的 HTML 且移除 script', () => {
    const wrapper = mount(KpSafeHtml, { props: { html: '<p>hi</p><script>alert(1)</script>' } });
    expect(wrapper.html()).toContain('hi');
    expect(wrapper.html()).not.toContain('<script');
  });
});

describe('App', () => {
  it('带 pinia + router 挂载成功并渲染标题与路由视图', async () => {
    await router.push('/');
    await router.isReady();
    const wrapper = mount(App, { global: { plugins: [createPinia(), router] } });
    expect(wrapper.text()).toContain('Knowl Pad');
    await flushPromises();
    expect(wrapper.find('.vault-list').exists()).toBe(true);
  });
});
