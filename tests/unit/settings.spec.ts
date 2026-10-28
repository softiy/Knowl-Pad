import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  applyEditorAppearance,
  applyTheme,
  contrastRatio,
  DEFAULT_THEME,
  loadTheme,
  resolveTheme,
  THEME_TOKENS,
  tokensFor,
} from '@core/theme';
import { SETTING_CATEGORIES, SETTING_ITEMS, useSettingsStore } from '@features/settings';
import { COMMANDS, searchCommands } from '@features/palette';

vi.mock('@core/ipc/commands', () => ({
  preferenceGet: vi.fn(async (keys: string[]) => ({ values: Object.fromEntries(keys.map((k) => [k, undefined])) })),
}));
vi.mock('@core/ipc/preferences', () => ({
  schedulePreferenceSet: vi.fn(),
}));

const root = process.cwd();

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
