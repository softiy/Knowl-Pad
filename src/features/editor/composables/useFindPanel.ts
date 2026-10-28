/**
 * 查找/替换面板的开关与聚焦（FR-SEARCH-30/31）。
 *
 * 抽出来的理由：EditorView 的 setup 已到 CODE-11 的函数行数线，把"打开哪个模式 + 聚焦哪个输入框 +
 * 关闭后把焦点还给编辑器"这三件事独立出来，组件只保留接线。
 */
import { nextTick, ref, type Ref } from 'vue';

export interface FindPanelHandle { focusInput(): void }

export function useFindPanel(host: Ref<HTMLElement | null>) {
  const findOpen = ref(false);
  const findMode = ref<'find' | 'replace'>('find');
  const findPanel = ref<FindPanelHandle | null>(null);

  function openFind(mode: 'find' | 'replace'): void {
    findOpen.value = true;
    findMode.value = mode;
    void nextTick(() => findPanel.value?.focusInput());
  }

  function toggleFind(): void {
    if (findOpen.value) findOpen.value = false;
    else openFind('replace');
  }

  /** Esc 关闭后把焦点还给编辑器（FR-SEARCH-30）。 */
  function closeFind(): void {
    findOpen.value = false;
    host.value?.focus();
  }

  return { findOpen, findMode, findPanel, openFind, toggleFind, closeFind };
}
