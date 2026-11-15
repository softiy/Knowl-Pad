/**
 * 查找/替换面板的开关与聚焦（FR-SEARCH-30/31）。
 *
 * 抽出来的理由：EditorView 的 setup 已到 CODE-11 的函数行数线，把"打开哪个模式 + 聚焦哪个输入框 +
 * 关闭后把焦点还给编辑器"这三件事独立出来，组件只保留接线。
 */
import { nextTick, ref } from 'vue';

export interface FindPanelHandle {
  focusInput(): void;
}

/** 只依赖"把焦点还给编辑区"这一个动作，便于单测与换内核（M8）。 */
export interface FocusTarget {
  focusEditor(): void;
}

export function useFindPanel(editor: () => FocusTarget | null) {
  const findOpen = ref(false);
  const findMode = ref<'find' | 'replace'>('find');
  const findPanel = ref<FindPanelHandle | null>(null);

  function openFind(mode: 'find' | 'replace'): void {
    findOpen.value = true;
    findMode.value = mode;
    void nextTick(() => findPanel.value?.focusInput());
  }

  function toggleFind(): void {
    if (findOpen.value) closeFind();
    else openFind('replace');
  }

  /** Esc/关闭后把焦点还给编辑器（FR-SEARCH-30）。 */
  function closeFind(): void {
    findOpen.value = false;
    // 必须聚焦**内核可编辑元素**：旧实现 focus() 的是外层宿主 div（不可聚焦），等于什么都没做（10.3）。
    editor()?.focusEditor();
  }

  return { findOpen, findMode, findPanel, openFind, toggleFind, closeFind };
}
