import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ref } from 'vue';
import { mount } from '@vue/test-utils';
import {
  applyEditorAppearance,
  applyTheme,
  contrastRatio,
  DEFAULT_THEME,
  loadTheme,
  resolveTheme,
  THEME_TOKENS,
  tokensFor,
  watchSystemTheme,
} from '@core/theme';
import { preferenceGet } from '@core/ipc/commands';
import { schedulePreferenceSet } from '@core/ipc/preferences';
import { SETTING_CATEGORIES, SETTING_ITEMS, SettingsPanel, useSettingsStore } from '@features/settings';
import { COMMANDS, searchCommands } from '@features/palette';

vi.mock('@core/ipc/commands', () => ({
  preferenceGet: vi.fn(async (keys: string[]) => ({ values: Object.fromEntries(keys.map((k) => [k, undefined])) })),
}));
vi.mock('@core/ipc/preferences', () => ({
  schedulePreferenceSet: vi.fn(),
}));

const root = process.cwd();

/** 单测默认：所有偏好键都"没有存过"（= 首次启动）。 */
const emptyPreferenceGet = async (keys: string[] = []): Promise<{ values: Record<string, unknown> }> => ({
  values: Object.fromEntries(keys.map((key) => [key, undefined])),
});

/** jsdom 的 matchMedia 不会真的匹配媒体查询，系统配色必须用可控替身注入。 */
type MediaListener = (event: MediaQueryListEvent) => void;

interface MatchMediaStub {
  emit(matches: boolean): void;
  listenerCount(): number;
}

function stubMatchMedia(initialDark: boolean): MatchMediaStub {
  const listeners: MediaListener[] = [];
  const query = {
    matches: initialDark,
    addEventListener: (_type: 'change', listener: MediaListener) => {
      listeners.push(listener);
    },
    removeEventListener: (_type: 'change', listener: MediaListener) => {
      const index = listeners.indexOf(listener);
      if (index >= 0) listeners.splice(index, 1);
    },
  };
  window.matchMedia = vi.fn(() => query as unknown as MediaQueryList) as unknown as typeof window.matchMedia;
  return {
    emit(matches: boolean): void {
      query.matches = matches;
      for (const listener of [...listeners]) listener({ matches } as MediaQueryListEvent);
    },
    listenerCount: () => listeners.length,
  };
}

const originalMatchMedia = window.matchMedia;

afterEach(() => {
  vi.mocked(preferenceGet).mockReset();
  vi.mocked(preferenceGet).mockImplementation(emptyPreferenceGet);
  vi.mocked(schedulePreferenceSet).mockClear();
  window.matchMedia = originalMatchMedia;
  delete document.documentElement.dataset.theme;
  document.documentElement.style.removeProperty('--kp-editor-font-size');
  document.documentElement.style.removeProperty('--kp-zoom');
});

describe('设置页分类（FR-SET-01/02，逐条对照 PRD §4.11.3）', () => {
  it('九个分类齐全，且顺序与 PRD 一致', () => {
    expect(SETTING_CATEGORIES).toEqual(['编辑器', '外观', '文件与链接', '搜索', '图谱', '回收站', '更新', '索引', '隐私']);
  });

  it('每一项都有默认值与作用域；主题默认「跟随系统」、字号默认 16px', () => {
    expect(SETTING_ITEMS.length).toBeGreaterThanOrEqual(33);
    for (const item of SETTING_ITEMS) {
      expect(item.defaultText.length).toBeGreaterThan(0);
      expect(['global', 'vault']).toContain(item.scope);
    }
    const theme = SETTING_ITEMS.find((i) => i.id === 'appearance.theme')!;
    expect(theme.defaultText).toBe('跟随系统');
    const font = SETTING_ITEMS.find((i) => i.id === 'appearance.editorFontSize')!;
    expect(font.defaultText).toBe('16px');
    // 本轮真正接上的四项
    expect(SETTING_ITEMS.filter((i) => i.wired !== undefined).map((i) => i.wired)).toEqual([
      'autosaveDelayMs', 'theme', 'editorFontSize', 'zoomPercent', 'showHidden',
    ]);
  });
});

describe('主题系统（FR-SET-20/21）', () => {
  beforeEach(() => applyTheme('light'));

  it('light/dark/system：system 依据系统偏好解析', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });

  it('applyTheme 写 <html data-theme>，无需重载即切换（FR-SET-21）', () => {
    applyTheme('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
    applyTheme('light');
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('两套主题的令牌不同（深色不是浅色的复制）', () => {
    const light = tokensFor('light');
    const dark = tokensFor('dark');
    expect(light.bg).not.toBe(dark.bg);
    expect(light.fg).not.toBe(dark.fg);
    expect(light.keyword).not.toBe(dark.keyword);
  });

  it('**可算的对比度断言**（FR-SET-25）：正文/次要文本/全部代码高亮色 ≥ 4.5:1', () => {
    for (const mode of ['light', 'dark'] as const) {
      const t = tokensFor(mode);
      expect(contrastRatio(t.fg, t.bg), mode + ' 正文').toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(t.muted, t.bg), mode + ' 次要').toBeGreaterThanOrEqual(4.5);
      for (const key of ['keyword', 'string', 'comment', 'number', 'literal', 'variable', 'heading', 'code', 'link', 'url']) {
        expect(contrastRatio(t[key]!, t.codeBg), mode + ' 高亮 ' + key).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('主题持久化走 preference（FR-SET-03/AC-SET-01）；坏值回落到默认', async () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    await store.init();
    store.setTheme('dark');
    const { schedulePreferenceSet } = await import('@core/ipc/preferences');
    expect(schedulePreferenceSet).toHaveBeenCalledWith({ 'appearance.theme': 'dark' });
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(DEFAULT_THEME).toBe('system');

    const { preferenceGet } = await import('@core/ipc/commands');
    vi.mocked(preferenceGet).mockResolvedValueOnce({ values: { 'appearance.theme': 'nonsense' } } as never);
    expect(await loadTheme()).toBe('system');
  });

  it('字号/缩放写根节点 CSS 变量（FR-SET-22/23），并夹取范围', () => {
    applyEditorAppearance({ fontSize: 18, zoom: 120 });
    expect(document.documentElement.style.getPropertyValue('--kp-editor-font-size')).toBe('18px');
    expect(document.documentElement.style.getPropertyValue('--kp-zoom')).toBe('1.2');

    setActivePinia(createPinia());
    const store = useSettingsStore();
    store.setFontSize(99);
    expect(store.fontSize).toBe(24);
    store.setZoom(10);
    expect(store.zoom).toBe(80);
  });
});

describe('主题令牌落地（DEBT-18：高亮配色由变量驱动，不硬编码）', () => {
  const tokensCss = readFileSync(join(root, 'src/core/theme/tokens.css'), 'utf8');
  const markdownVue = readFileSync(join(root, 'src/features/editor/components/MarkdownView.vue'), 'utf8');

  it('tokens.css 让 CodeMirror 编辑区字号跟随 --kp-editor-font-size（8.6 编辑态跟随）', () => {
    // md-editor-v3 把 CodeMirror 字号写死成 14px；不覆盖时字号设置只影响阅读态。
    expect(tokensCss).toMatch(/#app \.cm-editor[\s\S]{0,200}var\(--kp-editor-font-size/);
  });

  it('tokens.css 为两套主题定义了全部高亮变量', () => {
    for (const theme of [':root', "[data-theme='dark']"]) {
      const block = tokensCss.slice(tokensCss.indexOf(theme), tokensCss.indexOf(theme) + 1800);
      for (const key of ['--kp-hl-keyword', '--kp-hl-string', '--kp-hl-comment', '--kp-hl-number', '--kp-hl-literal', '--kp-hl-var', '--kp-hl-heading', '--kp-hl-code', '--kp-hl-link', '--kp-hl-url']) {
        expect(block, theme + ' 缺少 ' + key).toContain(key + ':');
      }
    }
  });

  it('MarkdownView 的 kp-hl-* 颜色全部来自 var(--kp-hl-*)，没有残留硬编码 hex', () => {
    const hlRules = markdownVue.split('\n').filter((line) => line.includes('.kp-hl-') || line.includes('pre.kp-pre'));
    expect(hlRules.length).toBeGreaterThanOrEqual(12);
    for (const rule of hlRules) {
      expect(rule).not.toMatch(/color:\s*#[0-9a-f]{3,6}/i);
      expect(rule).not.toMatch(/background:\s*#[0-9a-f]{3,6}/i);
    }
    expect(markdownVue).toContain('var(--kp-hl-keyword)');
    expect(markdownVue).toContain('var(--kp-code-bg)');
  });

  it('THEME_TOKENS 与 tokens.css 的取值一致（防两处漂移）', () => {
    for (const mode of ['light', 'dark'] as const) {
      const t = THEME_TOKENS[mode];
      for (const [key, value] of Object.entries({ keyword: t.keyword, string: t.string, comment: t.comment, number: t.number, literal: t.literal, variable: t.variable, heading: t.heading, code: t.code, link: t.link, url: t.url })) {
        expect(tokensCss.toLowerCase(), mode + '/' + key).toContain(value.toLowerCase());
      }
    }
  });
});

describe('命令面板新增两条命令（批次 8）', () => {
  it('「打开设置」「切换主题」可被搜到', () => {
    const names = COMMANDS.map((c) => c.name);
    expect(names).toContain('打开设置');
    expect(names).toContain('切换主题（浅色/深色）');
    expect(searchCommands('shezhi', COMMANDS)[0]?.name).toBe('打开设置');
    expect(searchCommands('zhuti', COMMANDS)[0]?.name).toBe('切换主题（浅色/深色）');
  });

  it('执行体会调用注入的 openSettings / toggleTheme', async () => {
    const calls: string[] = [];
    const settings = COMMANDS.find((c) => c.id === 'palette.settings')!;
    const theme = COMMANDS.find((c) => c.id === 'palette.toggle-theme')!;
    await settings.run({ openSettings: () => { calls.push('settings'); } } as never);
    await theme.run({ toggleTheme: () => { calls.push('theme'); } } as never);
    expect(calls).toEqual(['settings', 'theme']);
  });
});


describe('AC-SET-02：Vault 级设置隔离（vault_state，不是 preference）', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('作用域标记与实际写入路径一致：files.showHidden 标 Vault 且接线为 showHidden', () => {
    const item = SETTING_ITEMS.find((i) => i.id === 'files.showHidden')!;
    expect(item.scope).toBe('vault');
    expect(item.wired).toBe('showHidden');
    // 全局项不得被标成 vault（反之亦然）
    for (const global of SETTING_ITEMS.filter((i) => i.scope === 'global')) {
      expect(['theme', 'autosaveDelayMs', 'editorFontSize', 'zoomPercent', undefined]).toContain(global.wired);
    }
  });

  it('Vault A 打开隐藏文件、切到 B 为关、切回 A 仍为开（不串）', async () => {
    const store = useSettingsStore();
    // 用一个「按 Vault 分桶」的假 vault_state 模拟后端（键与生产一致：ui.showHiddenFiles）
    const perVault = new Map<string, boolean>();
    let currentVault = 'A';
    store.configureVaultScope({
      read: async () => perVault.get(currentVault) ?? false,
      write: async (value) => {
        perVault.set(currentVault, value);
      },
    });

    await store.loadVaultScope();
    expect(store.showHidden).toBe(false);
    await store.setShowHidden(true);
    expect(perVault.get('A')).toBe(true);
    expect(store.showHidden).toBe(true);

    currentVault = 'B';
    await store.loadVaultScope();
    expect(store.showHidden, '切到 B 不得看到 A 的值').toBe(false);
    await store.setShowHidden(false);

    currentVault = 'A';
    await store.loadVaultScope();
    expect(store.showHidden, '切回 A 仍保留 A 的值').toBe(true);
  });

  it('同一时刻只写当前 Vault 的桶（不写全局 preference）', async () => {
    const { schedulePreferenceSet } = await import('@core/ipc/preferences');
    vi.mocked(schedulePreferenceSet).mockClear();
    const store = useSettingsStore();
    const perVault = new Map<string, boolean>();
    store.configureVaultScope({
      read: async () => perVault.get('A') ?? false,
      write: async (value) => {
        perVault.set('A', value);
      },
    });
    await store.setShowHidden(true);
    expect(perVault.get('A')).toBe(true);
    expect(schedulePreferenceSet, 'Vault 级设置不得走全局偏好').not.toHaveBeenCalledWith(
      expect.objectContaining({ 'files.showHidden': true }),
    );
  });
});
describe('启动初始化：从全局库恢复外观（8.6 回归）', () => {
  it('init 在挂载前把主题/字号/缩放 apply 到 document，并把越界旧值夹取回允许范围', async () => {
    vi.mocked(preferenceGet).mockImplementation(async (keys: string[] = []) => ({
      values: Object.fromEntries(keys.map((key) => [key, ({
        'appearance.theme': 'dark',
        'appearance.editorFontSize': 20,
        'appearance.zoomPercent': 125,
        'editor.autosaveDelayMs': 2000,
      } as Record<string, unknown>)[key]])),
    }));

    setActivePinia(createPinia());
    const store = useSettingsStore();
    await store.init();

    expect(document.documentElement.dataset.theme, '主题在首次渲染前已 apply').toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--kp-editor-font-size')).toBe('20px');
    expect(document.documentElement.style.getPropertyValue('--kp-zoom')).toBe('1.25');
    expect(store.fontSize).toBe(20);
    expect(store.zoom).toBe(125);
    expect(store.autosaveDelayMs, '自动保存防抖也随启动恢复').toBe(2000);

    // 手改库/旧版本写坏的越界值：必须夹取，不能把界面撑坏
    vi.mocked(preferenceGet).mockImplementation(async (keys: string[] = []) => ({
      values: Object.fromEntries(keys.map((key) => [key, ({
        'appearance.theme': 'nonsense',
        'appearance.editorFontSize': 99,
        'appearance.zoomPercent': 10,
      } as Record<string, unknown>)[key]])),
    }));
    const second = createPinia();
    setActivePinia(second);
    const store2 = useSettingsStore();
    await store2.init();
    expect(store2.theme, '坏主题回落默认（跟随系统）').toBe('system');
    expect(store2.fontSize).toBe(24);
    expect(store2.zoom).toBe(80);
    expect(document.documentElement.style.getPropertyValue('--kp-editor-font-size')).toBe('24px');
    expect(document.documentElement.style.getPropertyValue('--kp-zoom')).toBe('0.8');
  });
});

describe('跟随系统主题（8.5 回归）', () => {
  it('watchSystemTheme 监听系统深浅色变化，注销后不再回调', () => {
    const media = stubMatchMedia(false);
    const seen: boolean[] = [];
    const unwatch = watchSystemTheme((dark) => seen.push(dark));
    expect(media.listenerCount()).toBe(1);
    media.emit(true);
    media.emit(false);
    expect(seen).toEqual([true, false]);

    unwatch();
    expect(media.listenerCount()).toBe(0);
    media.emit(true);
    expect(seen).toEqual([true, false]);
  });

  it('旧 WebView 只有 addListener/removeListener 时走兜底（不能"以为监听了"）', () => {
    const listeners: MediaListener[] = [];
    window.matchMedia = vi.fn(() => ({
      matches: false,
      addListener: (listener: MediaListener) => listeners.push(listener),
      removeListener: (listener: MediaListener) => {
        const index = listeners.indexOf(listener);
        if (index >= 0) listeners.splice(index, 1);
      },
    }) as unknown as MediaQueryList) as unknown as typeof window.matchMedia;

    const seen: boolean[] = [];
    const unwatch = watchSystemTheme((dark) => seen.push(dark));
    listeners.forEach((listener) => listener({ matches: true } as MediaQueryListEvent));
    expect(seen).toEqual([true]);
    unwatch();
    expect(listeners).toEqual([]);
  });

  it('system 模式下系统切深色 ⇒ data-theme 跟随；切到 light 后注销监听（不覆盖手选）', async () => {
    const media = stubMatchMedia(false);
    setActivePinia(createPinia());
    const store = useSettingsStore();
    await store.init();
    expect(store.theme).toBe('system');
    expect(document.documentElement.dataset.theme).toBe('light');

    media.emit(true);
    expect(document.documentElement.dataset.theme, '系统切深色后界面必须跟随').toBe('dark');

    store.setTheme('light');
    expect(media.listenerCount(), '非 system 模式必须注销监听').toBe(0);
    media.emit(true);
    expect(document.documentElement.dataset.theme, '手选 light 不得被系统变化覆盖').toBe('light');
  });
});

describe('自动保存防抖单一来源（S2 回归）', () => {
  it('接线后展示值跟随注入源，写入走注入而不是再写一份 preference', () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    const current = ref(1000);
    const written: number[] = [];
    store.configureAutosave({
      get: () => current.value,
      set: (ms) => {
        current.value = ms;
        written.push(ms);
      },
    });

    expect(store.autosaveDelayMs).toBe(1000);
    current.value = 2000;
    expect(store.autosaveDelayMs, '工具栏改值后设置面板必须看到同一个值').toBe(2000);

    store.setAutosaveDelay(2500);
    expect(written).toEqual([2500]);
    expect(store.autosaveDelayMs).toBe(2500);
    expect(schedulePreferenceSet, '落盘由编辑器 store 唯一负责').not.toHaveBeenCalledWith({
      'editor.autosaveDelayMs': 2500,
    });
  });

  it('未接线时退回本地持久化，并夹取到 0.5–5s', () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    store.setAutosaveDelay(9999);
    expect(store.autosaveDelayMs).toBe(5000);
    expect(schedulePreferenceSet).toHaveBeenCalledWith({ 'editor.autosaveDelayMs': 5000 });
    store.setAutosaveDelay(100);
    expect(store.autosaveDelayMs).toBe(500);
  });

  it('设置面板的自动保存输入显示编辑器当前值，修改走注入（S2 双源回归）', async () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    const current = ref(1000);
    const written: number[] = [];
    store.configureAutosave({
      get: () => current.value,
      set: (ms) => {
        written.push(ms);
        current.value = ms;
      },
    });
    store.open();
    const wrapper = mount(SettingsPanel);
    const input = wrapper.get('[data-testid="settings-autosave"]');
    expect((input.element as HTMLInputElement).value).toBe('1000');

    current.value = 2000;
    await wrapper.vm.$nextTick();
    expect((input.element as HTMLInputElement).value, '工具栏改值后面板同步').toBe('2000');

    (input.element as HTMLInputElement).value = '2500';
    await input.trigger('change');
    expect(written).toEqual([2500]);
    wrapper.unmount();
  });
});

describe('8.7 命令面板：切换主题 / 打开设置（防回归）', () => {
  it('中文短语「设置」「打开设置」「切换主题」都能命中对应命令', () => {
    expect(searchCommands('设置', COMMANDS).map((c) => c.id)).toContain('palette.settings');
    expect(searchCommands('打开设置', COMMANDS)[0]?.id).toBe('palette.settings');
    expect(searchCommands('切换主题', COMMANDS)[0]?.id).toBe('palette.toggle-theme');
  });

  it('「切换主题」执行体走 settings store：data-theme 立即变且写回 preference', () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    applyTheme('light');
    const command = COMMANDS.find((c) => c.id === 'palette.toggle-theme')!;

    void command.run({ toggleTheme: () => store.toggleTheme() } as never);
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(store.theme).toBe('dark');
    expect(schedulePreferenceSet).toHaveBeenCalledWith({ 'appearance.theme': 'dark' });

    void command.run({ toggleTheme: () => store.toggleTheme() } as never);
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(schedulePreferenceSet).toHaveBeenCalledWith({ 'appearance.theme': 'light' });
  });

  it('「打开设置」执行体会打开设置面板；两条命令都在 registry 里', async () => {
    setActivePinia(createPinia());
    const store = useSettingsStore();
    expect(store.isOpen).toBe(false);
    const command = COMMANDS.find((c) => c.id === 'palette.settings')!;
    await command.run({ openSettings: () => store.open() } as never);
    expect(store.isOpen).toBe(true);
    const ids = COMMANDS.map((c) => c.id);
    expect(ids).toContain('palette.settings');
    expect(ids).toContain('palette.toggle-theme');
  });
});