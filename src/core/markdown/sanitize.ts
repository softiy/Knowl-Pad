import DOMPurify, { type Config } from 'dompurify';

/** 全项目唯一的净化配置；禁止使用 DOMPurify.setConfig()（红线 R-05 / SEC-01）。 */
export const SANITIZE_CONFIG: Config = Object.freeze({
  FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input', 'button', 'style', 'link', 'meta', 'base'],
  FORBID_ATTR: ['onerror', 'onclick', 'onload', 'onmouseover', 'onfocus', 'onsubmit', 'srcdoc', 'formaction'],
  ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto|asset|tauri|blob):|[^a-z]|[a-z+.-]+(?:[^a-z+.-:]|$))/i,
  ADD_ATTR: ['data-kp-link', 'data-kp-tag', 'data-kp-anchor', 'target', 'rel', 'referrerpolicy', 'loading'],
  KEEP_CONTENT: true,
});

/** 全项目唯一的 HTML 净化入口。 */
export function sanitizeHtml(dirty: string): string {
  return DOMPurify.sanitize(dirty, SANITIZE_CONFIG);
}

/** 启动自检：净化失效即抛错阻断启动（AC-SEC-01）。 */
export function selfTest(): void {
  const probes = [
    '<script>alert(1)</script>',
    '<img src=x onerror=alert(2)>',
    '<iframe src="https://evil.example"></iframe>',
    '<a href="javascript:alert(3)">x</a>',
    '<svg onload=alert(4)>',
    '<div onclick=alert(5)>x</div>',
  ];
  for (const probe of probes) {
    const out = sanitizeHtml(probe);
    if (/script|iframe|onerror|onclick|onload|javascript:/i.test(out)) {
      throw new Error('DOMPurify 自检失败：' + probe + ' -> ' + out);
    }
  }
}
