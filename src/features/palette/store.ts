/**
 * 命令面板的**状态机**（打开/过滤/键盘选中/执行）。
 *
 * 为什么放在 Pinia store 而不是组件里：快捷键（WorkspaceLayout）与面板组件不在同一棵树；
 * 打开/关闭必须由全局快捷键驱动。执行体由布局层 `configure()` 注入，store 不 import 任何 feature（FE-02）。
 */
import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import {
  COMMANDS,
  disabledReason,
  isEnabled,
  type PaletteActions,
  type PaletteCommand,
  type PaletteContext,
} from './registry';
import { PAGE_SIZE, createCommandIndex } from './search';

export const usePaletteStore = defineStore('palette', () => {
  const isOpen = ref(false);
  const query = ref('');
  const activeIndex = ref(0);
  const error = ref<string | null>(null);
  const context = ref<PaletteContext>({ hasActiveNote: false, hasVault: false, mode: 'edit', includeHidden: false });
  let actions: PaletteActions | null = null;
  const index = createCommandIndex(COMMANDS);

  /** 过滤后的候选（含不可用项：置灰 + 原因，见 FR-PALETTE-08 的展示口径）。 */
  const results = computed<PaletteCommand[]>(() => {
    const trimmed = query.value.trim();
    if (trimmed === '') return COMMANDS.slice(0, PAGE_SIZE);
    return index
      .search(trimmed)
      .slice(0, PAGE_SIZE)
      .map((hit) => hit.item);
  });

  const activeCommand = computed<PaletteCommand | null>(() => results.value[activeIndex.value] ?? null);

  function configure(next: PaletteActions): void {
    actions = next;
  }

  function open(next?: Partial<PaletteContext>): void {
    if (next !== undefined) context.value = { ...context.value, ...next };
    isOpen.value = true;
    query.value = '';
    activeIndex.value = 0;
    error.value = null;
  }

  function close(): void {
    isOpen.value = false;
    error.value = null;
  }

  function setQuery(value: string): void {
    query.value = value;
    activeIndex.value = 0;
  }

  function move(delta: number): void {
    const count = results.value.length;
    if (count === 0) return;
    activeIndex.value = (activeIndex.value + delta + count) % count;
  }

  /**
   * 执行第 `at` 条（默认当前选中）。返回是否成功。
   *
   * 语义（FR-PALETTE-07）：成功 ⇒ 关闭面板；失败 ⇒ **保持打开**并给出可见错误（R-07 不静默失败）。
   */
  async function execute(at: number = activeIndex.value): Promise<boolean> {
    const command = results.value[at];
    if (command === undefined) return false;
    if (!isEnabled(command, context.value)) {
      error.value = disabledReason(command, context.value);
      return false;
    }
    if (actions === null) {
      error.value = '命令面板尚未接线（内部错误）';
      return false;
    }
    try {
      await command.run(actions);
      close();
      return true;
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
      return false;
    }
  }

  return {
    isOpen,
    query,
    activeIndex,
    error,
    context,
    results,
    activeCommand,
    configure,
    open,
    close,
    setQuery,
    move,
    execute,
  };
});
