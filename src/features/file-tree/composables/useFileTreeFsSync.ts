/**
 * 文件树与后端文件系统事件（kp://fs/*）的**最小同步**（M5 冒烟 5.3 的最后一环）。
 *
 * 为什么需要它：文件树是"快照 + 懒加载缓存"，而排序是对**缓存**排序 —— 不订阅事件时，
 * 运行期外部改动（改 mtime / 新建 / 删除 / 重命名）永远不进树，切到「修改时间」仍是打开时的旧顺序。
 * 同步规则（只做最小同步，不重建整棵树）：
 * - modified：事件载荷已带磁盘 mtime ⇒ 直接更新该节点并重排（也顺带覆盖应用内保存后的顺序刷新）；
 * - created：索引行已由监听器写好 ⇒ 定向重拉父层（只动已加载的层）；
 * - removed / renamed：本地摘除 / 改名（与 fileOps 的本地同步是同一套幂等函数）。
 *
 * 生命周期跟着组件：卸载时注销全部监听（ED-01：组件不留下全局监听）。
 */
import { onMounted, onUnmounted } from 'vue';
import { onFsCreated, onFsModified, onFsRemoved, onFsRenamed } from '@core/ipc/events';
import { asKpError } from '@core/ipc/errors';
import type { useFileTreeStore } from '../stores/fileTree';

/** 订阅需要的 store 能力（用 ReturnType 保证与真实 store 同步，不复制 API 形状）。 */
type FileTreeStore = ReturnType<typeof useFileTreeStore>;

export function useFileTreeFsSync(store: FileTreeStore): void {
  let unlisten: (() => void)[] = [];
  /** 组件已卸载的守卫：订阅是异步的，若在 await 期间卸载，回来后必须自己注销（否则泄漏）。 */
  let disposed = false;

  async function bind(): Promise<void> {
    try {
      const bindings = await Promise.all([
        onFsModified((payload) => store.applyMtime(payload.relPath, payload.mtimeMs)),
        onFsCreated((payload) => {
          void store.refreshPath(payload.relPath);
        }),
        onFsRemoved((payload) => {
          store.pathRemoved(payload.relPath);
        }),
        onFsRenamed((payload) => {
          store.pathMoved(payload.from, payload.to);
        }),
      ]);
      if (disposed) {
        for (const off of bindings) off();
        return;
      }
      unlisten = bindings;
    } catch (err) {
      // 事件不可用（例如非 Tauri 环境）只影响"实时更新"，不影响树本身：如实记因，不静默（R-15）
      store.error = asKpError(err).message;
    }
  }

  onMounted(() => {
    void bind();
  });

  onUnmounted(() => {
    disposed = true;
    for (const off of unlisten) off();
    unlisten = [];
  });
}
