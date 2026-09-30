import { describe, expect, it } from 'vitest';
import { clamp, formatBytes, toHex } from '@core/utils/hex';

describe('toHex', () => {
  it('编码为空字符串', () => expect(toHex(new Uint8Array([]))).toBe(''));
  it('小写补零', () => expect(toHex(new Uint8Array([0, 15, 16, 255]))).toBe('000f10ff'));
  it('等价于已知 SHA-256 向量', () =>
    expect(toHex(new Uint8Array([0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14]))).toBe('e3b0c44298fc1c14'));
});

describe('clamp', () => {
  it('下界', () => expect(clamp(-5, 0, 10)).toBe(0));
  it('上界', () => expect(clamp(50, 0, 10)).toBe(10));
  it('区间内', () => expect(clamp(5, 0, 10)).toBe(5));
  it('NaN 取 min', () => expect(clamp(Number.NaN, 2, 10)).toBe(2));
});

describe('formatBytes', () => {
  it('非法值', () => expect(formatBytes(-1)).toBe('0 B'));
  it('字节', () => expect(formatBytes(512)).toBe('512 B'));
  it('KB', () => expect(formatBytes(2048)).toBe('2.0 KB'));
  it('MB', () => expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB'));
  it('TB 上限', () => expect(formatBytes(1024 ** 5)).toBe('1024.0 TB'));
});
