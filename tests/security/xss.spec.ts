import { describe, expect, it } from 'vitest';
import { sanitizeHtml, selfTest } from '@core/markdown/sanitize';

// AC-EDITOR-01 / AC-SEC-01：安全测试集（发布门禁）
const PROBES = [
  { name: 'script 标签', input: '<script>alert(1)</script>', forbid: /script/i },
  { name: 'img onerror', input: '<img src=x onerror=alert(2)>', forbid: /onerror/i },
  { name: 'javascript 协议', input: '<a href="javascript:alert(3)">x</a>', forbid: /javascript:/i },
  { name: 'iframe', input: '<iframe src="http://evil.com"></iframe>', forbid: /iframe/i },
  { name: 'svg onload', input: '<svg onload=alert(4)>', forbid: /onload/i },
  { name: 'form 注入', input: '<form action="/x"><input name="a"></form>', forbid: /<form|<input/i },
  { name: 'style 注入', input: '<style>body{display:none}</style>', forbid: /<style/i },
  { name: 'object/embed', input: '<object data="x"></object><embed src="y">', forbid: /object|embed/i },
  { name: 'srcdoc', input: '<iframe srcdoc="<script>1</script>"></iframe>', forbid: /srcdoc/i },
  { name: 'base 标签', input: '<base href="http://evil.com/">', forbid: /<base/i },
];

describe('AC-SEC-01 内容渲染安全（发布门禁）', () => {
  for (const probe of PROBES) {
    it('拦截：' + probe.name, () => {
      expect(sanitizeHtml(probe.input)).not.toMatch(probe.forbid);
    });
  }
  it('启动自检必须通过（净化失效即阻断启动）', () => {
    expect(() => { selfTest(); }).not.toThrow();
  });
});
