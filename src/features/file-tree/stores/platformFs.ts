/**
 * 平台事实：当前文件系统是否**区分大小写**。
 *
 * 来源是 Rust `platform::case_insensitive_fs`（经 `vault_current` 的 `caseInsensitiveFs` 字段送回），
 * 前端**不**硬编码平台判断（R-09：不得硬编码平台路径或分隔符）。
 *
 * 消费方：移动前"目标与当前是否同一个文件"的校验（M5 冒烟 5.4）—— 在 Windows/macOS 默认
 * 不区分大小写的盘上，`归档/A.md` 与 `归档/a.md` 是同一个文件，必须按"相同"拦截。
 */
import { asKpError } from '@core/ipc/errors';
import { vaultCurrent } from '@core/ipc/commands';

/** 读平台大小写口径；取不到时按"不区分大小写"处理并留痕（R-15）。 */
export async function caseInsensitiveFs(): Promise<boolean> {
  try {
    return (await vaultCurrent())?.caseInsensitiveFs ?? true;
  } catch (err) {
    // 降级方向：按不区分大小写 ⇒ 最多多拦下一次不会产生变化的移动（界面有红字说明），
    // 不会发出注定 no-op 的命令。取不到平台事实的原因必须可见（R-15），故 warn。
    console.warn('读取平台大小写口径失败，按不区分大小写处理', asKpError(err).message);
    return true;
  }
}
