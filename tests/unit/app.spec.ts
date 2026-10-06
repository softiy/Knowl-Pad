import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { describe, expect, it, vi } from 'vitest';
import App from '@/App.vue';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';
import { bootstrap } from '@/app/bootstrap';
import { router } from '@/app/router';

vi.mock('@core/ipc/commands', () => ({
  vaultList: vi.fn().mockResolvedValue([]),
  vaultCurrent: vi.fn().mockResolvedValue(null),
}));

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
    // RT-01 守卫需要已激活的 pinia（真实应用在 router 之前安装 pinia）
    const pinia = createPinia();
    setActivePinia(pinia);
    await router.push('/welcome');
    await router.isReady();
    const wrapper = mount(App, { global: { plugins: [pinia, router] } });
    expect(wrapper.text()).toContain('Knowl Pad');
    await flushPromises();
    expect(wrapper.find('[data-testid="vault-welcome"]').exists()).toBe(true);
  });
});
