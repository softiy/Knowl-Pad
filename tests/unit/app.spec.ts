import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '@/App.vue';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';
import { bootstrap } from '@/app/bootstrap';
import { router } from '@/app/router';
import { installPreferenceFlushOnUnload } from '@core/ipc/preferences';

/** 上一次运行写进全局库的外观（模拟重启后库里已有的值；8.6 的回归夹具）。 */
const { persisted } = vi.hoisted(() => ({
  persisted: {
    'appearance.theme': 'dark',
    'appearance.editorFontSize': 20,
    'appearance.zoomPercent': 125,
    'editor.autosaveDelayMs': 2000,
  } as Record<string, unknown>,
}));

vi.mock('@core/ipc/commands', () => ({
  preferenceGet: vi.fn(async (keys: string[] = []) => ({
    values: Object.fromEntries(keys.map((key) => [key, persisted[key]])),
  })),
  preferenceSet: vi.fn(async () => undefined),
  vaultList: vi.fn().mockResolvedValue([]),
  vaultCurrent: vi.fn().mockResolvedValue(null),
}));

// 卸载 flush 钩子的真实行为由 preferences-debounce.spec 钉住；这里只验证启动编排调用它一次。
vi.mock('@core/ipc/preferences', () => ({
  installPreferenceFlushOnUnload: vi.fn(() => () => undefined),
  schedulePreferenceSet: vi.fn(),
}));

function resetAppearance(): void {
  delete document.documentElement.dataset.theme;
  document.documentElement.style.removeProperty('--kp-editor-font-size');
  document.documentElement.style.removeProperty('--kp-zoom');
}

afterEach(resetAppearance);

describe('bootstrap（启动编排：外观必须在 mount 前初始化）', () => {
  it('await bootstrap(pinia) 后主题/字号/缩放/自动保存已按持久化值恢复（8.6/S2 回归）', async () => {
    const pinia = createPinia();
    await bootstrap(pinia);

    expect(document.documentElement.dataset.theme, '主题在挂载前已 apply').toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--kp-editor-font-size')).toBe('20px');
    expect(document.documentElement.style.getPropertyValue('--kp-zoom')).toBe('1.25');
    expect(installPreferenceFlushOnUnload, '启动时注册卸载前 flush（S2 的另一半）').toHaveBeenCalledTimes(1);

    const { useSettingsStore } = await import('@features/settings');
    const settings = useSettingsStore(pinia);
    expect(settings.theme).toBe('dark');
    expect(settings.fontSize).toBe(20);
    expect(settings.zoom).toBe(125);
    expect(settings.autosaveDelayMs).toBe(2000);
  });

  it('自检通过：bootstrap 正常 resolve（失败会抛错阻断启动，AC-SEC-01）', async () => {
    await expect(bootstrap(createPinia())).resolves.toBeUndefined();
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
