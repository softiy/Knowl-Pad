/** 将字节序列编码为小写十六进制字符串（sha2 0.11 的 finalize() 不再实现 LowerHex，需自行编码）。 */
export function toHex(bytes: Uint8Array): string {
  let out = '';
  for (const b of bytes) out += b.toString(16).padStart(2, '0');
  return out;
}

/** 将数值限制在 [min, max] 区间内。 */
export function clamp(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) return min;
  return Math.min(Math.max(value, min), max);
}

/** 人类可读的字节数（1024 进制，最多一位小数）。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return i === 0 ? `${n} B` : `${n.toFixed(1)} ${units[i]}`;
}
