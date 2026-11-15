import type { Pinia } from 'pinia';
import { selfTest } from '@core/markdown/sanitize';
import { installPreferenceFlushOnUnload } from '@core/ipc/preferences';
import { useSettingsStore } from '@features/settings';

/**
 * 启动编排（`main.ts` 在 mount 之前 await 它）。
 *
 * 三步都不能省：
 * 1. 净化器自检：失败即抛错阻断启动（AC-SEC-01）；
 * 2. 偏好落盘钩子：关窗/隐藏前把 500ms 防抖队列 flush 掉，否则快速改设置后重启会丢（S2）；
 * 3. 外观初始化：主题/字号/缩放/自动保存防抖从全局库恢复并 apply 到 document。
 *    必须在首次渲染前完成：反序会先按默认主题渲染一帧再跳色（FR-SET-07/21），
 *    且设置面板会短暂显示默认值（真机 8.6 "重启回默认"的根因是这里从没被调用）。
 */
export async function bootstrap(pinia: Pinia): Promise<void> {
  selfTest();
  installPreferenceFlushOnUnload();
  await useSettingsStore(pinia).init();
}
