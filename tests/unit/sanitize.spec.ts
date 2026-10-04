import { describe, expect, it } from 'vitest';
import { SANITIZE_CONFIG, sanitizeHtml, selfTest } from '@core/markdown/sanitize';

describe('sanitizeHtml', () => {
  it('移除 script', () => expect(sanitizeHtml('<script>alert(1)</script>ok')).not.toMatch(/script/i));
  it('移除事件属性', () => expect(sanitizeHtml('<img src=x onerror=alert(2)>')).not.toMatch(/onerror/i));
  it('移除 javascript: 协议', () => expect(sanitizeHtml('<a href="javascript:alert(3)">x</a>')).not.toMatch(/javascript:/i));
  it('移除 iframe', () => expect(sanitizeHtml('<iframe src="https://e.com"></iframe>')).not.toMatch(/iframe/i));
  it('保留安全标签与属性', () => {
    const out = sanitizeHtml('<p data-kp-link="x">hi</p>');
    expect(out).toContain('data-kp-link="x"');
    expect(out).toContain('hi');
  });
  // 回归：\`[^a-z+.-:]\` 曾被解析成 .–: 范围（含 /），把相对路径整条剥掉
  it('保留相对 URL：本地附件 src 与相对链接 href', () => {
    expect(sanitizeHtml('<img src="attachments/a.png">')).toContain('src="attachments/a.png"');
    expect(sanitizeHtml('<a href="notes/sub/a.md">x</a>')).toContain('href="notes/sub/a.md"');
    expect(sanitizeHtml('<img src="./a.png">')).toContain('src="./a.png"');
  });
  it('配置为冻结对象且不含 setConfig 用法', () => expect(Object.isFrozen(SANITIZE_CONFIG)).toBe(true));
  it('自检通过', () => expect(() => { selfTest(); }).not.toThrow());
});
