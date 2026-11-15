/**
 * 文件操作**提示（notice）的控制器**：设置 + 自动清除 + 覆盖旧提示。
 *
 * 为什么必须自动清除（M5 冒烟 5.4/5.5）：布局层会**永久渲染** `ops.notice`（`WorkspaceLayout`
 * 的底部提示条），此前 store 只赋值、从不置空 —— 于是"已移动到 归档/待移动.md"这类提示
 * 一直挂在应用下方不消失。清除时机由 store 自己负责（布局层无需知道时长）。
 *
 * 时长口径：成功提示约 4s（够看清一句话）；错误提示 8s（用户可能需要读原因 / 转述）。
 * 新提示覆盖旧提示并**重置计时**，避免两条提示的定时器互相打架。
 */
import { ref, type Ref } from 'vue';

/** 成功提示的展示时长（毫秒）。 */
export const NOTICE_SUCCESS_MS = 4000;
/** 错误提示的展示时长（毫秒）：比成功长一倍，便于阅读原因。 */
export const NOTICE_ERROR_MS = 8000;

export type NoticeKind = 'success' | 'error';

export interface NoticeController {
  notice: Ref<string | null>;
  noticeKind: Ref<NoticeKind>;
  setNotice: (message: string, kind?: NoticeKind) => void;
  clearNotice: () => void;
}

/** 建立一个提示控制器（每个 fileOps store 一个；切库时由 clearNotice() 清掉串库提示）。 */
export function createNoticeController(): NoticeController {
  const notice = ref<string | null>(null);
  const noticeKind = ref<NoticeKind>('success');
  let timer: ReturnType<typeof setTimeout> | null = null;

  function clearTimer(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  /** 立即清空提示并取消计时（切库时调用，M5 冒烟「切库后串提示」）。 */
  function clearNotice(): void {
    clearTimer();
    notice.value = null;
  }

  function setNotice(message: string, kind: NoticeKind = 'success'): void {
    clearTimer();
    notice.value = message;
    noticeKind.value = kind;
    timer = setTimeout(
      () => {
        notice.value = null;
        timer = null;
      },
      kind === 'error' ? NOTICE_ERROR_MS : NOTICE_SUCCESS_MS,
    );
  }

  return { notice, noticeKind, setNotice, clearNotice };
}
