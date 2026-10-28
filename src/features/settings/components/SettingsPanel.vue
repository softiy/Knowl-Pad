<script setup lang="ts">
/**
 * 设置面板（FR-SET-01/02/07；FR-SET-24：**不**提供自定义 CSS 注入）。
 *
 * 分类与顺序逐条来自 PRD §4.11.3（catalog.ts）；本轮真正接上行为的是四项：
 * 主题、编辑器字号、界面缩放、自动保存防抖 —— 其余显示默认值并明确标注「默认值（未接入）」，
 * 不把不可调的项做成看起来能调的控件（R-15 同族：不静默假装）。
 */
import { computed } from 'vue';
import { storeToRefs } from 'pinia';
import { SETTING_CATEGORIES, itemsOf } from '../catalog';
import { useSettingsStore } from '../store';

const settings = useSettingsStore();
const { isOpen, theme, fontSize, zoom, autosaveDelayMs, showHidden } = storeToRefs(settings);

const categories = computed(() => SETTING_CATEGORIES.map((name) => ({ name, items: itemsOf(name) })));

function onThemeChange(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  settings.setTheme(value === 'light' || value === 'dark' ? value : 'system');
}
function onFontSize(event: Event): void {
  settings.setFontSize(Number((event.target as HTMLInputElement).value));
}
function onZoom(event: Event): void {
  settings.setZoom(Number((event.target as HTMLInputElement).value));
}
function onShowHidden(event: Event): void {
  void settings.setShowHidden((event.target as HTMLInputElement).checked);
}
function onAutosave(event: Event): void {
  settings.setAutosaveDelay(Number((event.target as HTMLInputElement).value));
}
</script>

<template>
  <div v-if="isOpen" class="kp-settings" data-testid="settings" @mousedown.self="settings.close()">
    <section class="kp-settings__panel" role="dialog" aria-modal="true" aria-label="设置">
      <header class="kp-settings__head">
        <h2 class="kp-settings__title">设置</h2>
        <button type="button" class="kp-settings__close" data-testid="settings-close" @click="settings.close()">关闭</button>
      </header>
      <div class="kp-settings__body">
        <section v-for="group in categories" :key="group.name" class="kp-settings__group" :data-testid="`settings-group-${group.name}`">
          <h3 class="kp-settings__group-title">{{ group.name }}</h3>
          <div v-for="item in group.items" :key="item.id" class="kp-settings__row" :data-testid="`setting-${item.id}`">
            <span class="kp-settings__label">{{ item.label }}</span>
            <span class="kp-settings__scope" :title="item.scope === 'global' ? '全局（跨 Vault）' : '仅当前 Vault'">
              {{ item.scope === 'global' ? '全局' : 'Vault' }}
            </span>
            <template v-if="item.wired === 'theme'">
              <select class="kp-settings__control" data-testid="settings-theme" :value="theme" @change="onThemeChange">
                <option value="system">跟随系统</option>
                <option value="light">浅色</option>
                <option value="dark">深色</option>
              </select>
            </template>
            <template v-else-if="item.wired === 'editorFontSize'">
              <input class="kp-settings__control" data-testid="settings-font-size" type="number" min="12" max="24" :value="fontSize" @change="onFontSize">
              <span class="kp-settings__unit">px</span>
            </template>
            <template v-else-if="item.wired === 'zoomPercent'">
              <input class="kp-settings__control" data-testid="settings-zoom" type="number" min="80" max="150" step="5" :value="zoom" @change="onZoom">
              <span class="kp-settings__unit">%</span>
            </template>
            <template v-else-if="item.wired === 'showHidden'">
              <input class="kp-settings__control" data-testid="settings-show-hidden" type="checkbox" :checked="showHidden" @change="onShowHidden">
            </template>
            <template v-else-if="item.wired === 'autosaveDelayMs'">
              <input class="kp-settings__control" data-testid="settings-autosave" type="number" min="500" max="5000" step="100" :value="autosaveDelayMs" @change="onAutosave">
              <span class="kp-settings__unit">ms</span>
            </template>
            <template v-else>
              <span class="kp-settings__default" :data-testid="`setting-default-${item.id}`">默认值：{{ item.defaultText }}</span>
              <span class="kp-settings__badge">默认值（未接入）</span>
            </template>
          </div>
        </section>
      </div>
      <footer class="kp-settings__foot">
        主题与外观即时生效（FR-SET-07）；作用域「全局」写 preference 表（500ms 防抖批量）。
      </footer>
    </section>
  </div>
</template>

<style scoped>
.kp-settings { position: fixed; inset: 0; z-index: 1000; display: flex; align-items: stretch; justify-content: flex-end; background: rgba(0, 0, 0, 0.35); }
.kp-settings__panel { width: min(560px, 96vw); display: flex; flex-direction: column; background: var(--kp-bg); color: var(--kp-fg); border-left: 1px solid var(--kp-border); }
.kp-settings__head { display: flex; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--kp-border); }
.kp-settings__title { margin: 0; font-size: 15px; flex: 1; }
.kp-settings__close { font-size: 12px; }
.kp-settings__body { flex: 1; overflow-y: auto; padding: 8px 14px; }
.kp-settings__group { margin: 10px 0 16px; }
.kp-settings__group-title { margin: 0 0 6px; font-size: 12px; text-transform: none; color: var(--kp-muted); }
.kp-settings__row { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 13px; border-bottom: 1px dashed var(--kp-border); }
.kp-settings__label { flex: 1; }
.kp-settings__scope { font-size: 11px; color: var(--kp-muted); }
.kp-settings__control { width: 96px; font-size: 12px; }
.kp-settings__unit { font-size: 11px; color: var(--kp-muted); }
.kp-settings__default { font-size: 12px; color: var(--kp-muted); }
.kp-settings__badge { font-size: 10px; color: var(--kp-muted); border: 1px solid var(--kp-border); border-radius: 3px; padding: 0 4px; }
.kp-settings__foot { padding: 8px 14px; font-size: 11px; color: var(--kp-muted); border-top: 1px solid var(--kp-border); }
</style>
