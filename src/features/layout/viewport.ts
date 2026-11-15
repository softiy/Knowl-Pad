import { clampSidebarWidth } from './store';

/**
 * 窗口尺寸跟踪（FR-LAYOUT-04）。
 *
 * 为什么单独成模块：此前 `resize` 监听写在 watch(vaultId) 回调里，既**从不注销**
 * （切一次库多一个监听）又只在切库时注册（启动后拖窗口不会折叠）——真机 9.7 的直接原因。
 * 这里给出「注册即立即量一次 + 返回注销函数」的唯一正确形态。
 */
export function watchViewport(onWidth: (width: number) => void): () => void {
  const apply = (): void => onWidth(window.innerWidth);
  apply();
  window.addEventListener('resize', apply);
  return () => window.removeEventListener('resize', apply);
}

/**
 * 侧栏宽度拖拽（FR-LAYOUT-01）。
 *
 * 口径：`pointermove` 只做**预览**（onPreview，不落盘），`pointerup` 才提交一次（onCommit）；
 * 每次移动都夹取到 [240, 520]——拖出边界时宽度停在边界上，而不是攒着一个越界值。
 * 监听挂在 window 上（而不是 resizer 元素 + pointer capture）：鼠标移出细窄的 resizer 后仍要跟手，
 * 且 window 上收 pointerup 保证松手一定落盘（ED-01：提交与注销都是确定的）。
 */
export function beginSidebarResize(
  event: PointerEvent,
  startWidth: number,
  onPreview: (width: number) => void,
  onCommit: (width: number) => void,
): void {
  event.preventDefault();
  const startX = event.clientX;
  let pending: number | null = null;
  const onMove = (ev: PointerEvent): void => {
    pending = clampSidebarWidth(startWidth + (ev.clientX - startX));
    onPreview(pending);
  };
  const finish = (): void => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', finish);
    window.removeEventListener('pointercancel', finish);
    if (pending !== null) {
      onCommit(pending);
      pending = null;
    }
  };
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', finish);
  window.addEventListener('pointercancel', finish);
}
