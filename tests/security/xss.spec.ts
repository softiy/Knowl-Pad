import { describe, expect, it } from 'vitest';
import { sanitizeHtml, selfTest } from '@core/markdown/sanitize';
import { renderMarkdown } from '@core/markdown/renderer';

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

// AC-EDITOR-01 的**管线级**探针：笔记正文经完整渲染管线后同样不得出现危险内容
const PIPELINE_PROBES = [
  { name: 'markdown 中的 script 标签', input: '# 标题\n\n<script>alert(1)</script>', forbid: /script/i },
  { name: 'markdown 中的 img onerror', input: '![x](x.png)\n\n<img src=x onerror=alert(2)>', forbid: /onerror/i },
  // 链接非法协议时 markdown-it 会把它降级为**纯文本**（属性里必须没有 javascript:）
  {
    name: 'markdown 链接 javascript:',
    input: '[点击](javascript:alert(3))<a href="javascript:alert(4)">x</a>',
    forbid: /<a[^>]*javascript:/i,
  },
  { name: 'markdown 中的 iframe', input: '<iframe src="http://evil.com"></iframe>', forbid: /iframe/i },
  { name: '远程图片不得以 <img> 出现（SEC-08）', input: '![x](https://evil.example/x.png)', forbid: /<img[^>]*https:/i },
];

describe('AC-EDITOR-01 渲染管线安全（发布门禁）', () => {
  for (const probe of PIPELINE_PROBES) {
    it('拦截：' + probe.name, () => {
      expect(renderMarkdown(probe.input)).not.toMatch(probe.forbid);
    });
  }
  it('文本内容以安全形式可见（链接降级为纯文本或安全 URL）', () => {
    const html = renderMarkdown('[点击](javascript:alert(3))');
    expect(html).toContain('点击');
  });
});
