/**
 * 行定位的**纯 DOM 实现**（FR-LINK-13）。
 *
 * 单独成文件的原因有二：① 适配器文件已贴近 CODE-11 的 200 行线；
 * ② 这里是纯函数，可以直接单测"选中哪一行/越界怎么收敛"，不必mount 编辑器。
 *
 * 约定：line 为 **1-based**，与索引库的 line 同口径。
 * 返回是否真的定位成功 —— 拿不到可编辑元素时返回 false，由调用方**如实降级**。
 */
export function revealLineIn(
  host: HTMLElement | null,
  value: string,
  line: number,
): boolean {
  if (!host) return false;
  const ta = host.querySelector('textarea');
  if (!ta) return false;
  const lines = value.split('\n');
  if (lines.length === 0) return false;
  const idx = Math.min(Math.max(Math.trunc(line), 1), lines.length) - 1;
  let start = 0;
  for (let i = 0; i < idx; i += 1) start += lines[i].length + 1;
  const end = start + lines[idx].length;
  ta.focus();
  ta.setSelectionRange(start, end);
  // 按行高估算滚动位置，让目标行大致落在视口上三分之一处
  const lineHeight = Number.parseFloat(getComputedStyle(ta).lineHeight || '') || 20;
  ta.scrollTop = Math.max(0, idx * lineHeight - ta.clientHeight / 3);
  return true;
}
