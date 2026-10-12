import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { beforeEach, describe, expect, it } from 'vitest';
import { clearRenderCache, renderMarkdown, renderStats } from '@core/markdown/renderer';

describe('renderMarkdown（§7.2 管线）', () => {
  beforeEach(() => clearRenderCache());

  it('渲染 CommonMark 基础语法', () => {
    const html = renderMarkdown('# 标题\n\n**粗体** 与 `code`\n\n- 一\n- 二');
    expect(html).toContain('<h1>标题</h1>');
    expect(html).toContain('<strong>粗体</strong>');
    expect(html).toContain('<code>code</code>');
    expect(html).toContain('<li>一</li>');
  });

  it('允许原始 HTML，但必须先被净化', () => {
    const html = renderMarkdown('<b>保留</b> <script>alert(1)</script>');
    expect(html).toContain('<b>保留</b>');
    expect(html).not.toMatch(/script/i);
  });

  it('远程图片不进入 DOM：只留占位符与 URL（SEC-08）', () => {
    const html = renderMarkdown('![x](https://evil.example/a.png)');
    expect(html).not.toContain('<img');
    expect(html).toContain('data-kp-remote-src="https://evil.example/a.png"');
    expect(html).toContain('远程图片');
  });

  it('协议相对地址同样按远程处理', () => {
    const html = renderMarkdown('![x](//cdn.example/a.png)');
    expect(html).not.toContain('<img');
    expect(html).toContain('data-kp-remote-src="//cdn.example/a.png"');
  });

  it('本地附件走 resolveAsset 并补 no-referrer 与 lazy', () => {
    const html = renderMarkdown('![图](attachments/a.png)', {
      resolveAsset: (src) => `asset://localhost/vault/${src}`,
    });
    expect(html).toContain('src="asset://localhost/vault/attachments/a.png"');
    expect(html).toContain('referrerpolicy="no-referrer"');
    expect(html).toContain('loading="lazy"');
  });

  it('resolveAsset 返回 null 时保留原 src（仍然不加载远程）', () => {
    const html = renderMarkdown('![图](a.png)', { resolveAsset: () => null });
    expect(html).toContain('src="a.png"');
    expect(html).toContain('referrerpolicy="no-referrer"');
  });

  it('data: 内联图片被阻止（不留 src、不留载荷）', () => {
    // DOMPurify 对 img 的 DATA_URI_TAGS 默认放行 data:，故由图片策略显式阻止
    const html = renderMarkdown('<img src="data:image/svg+xml,<svg onload=alert(1)>">');
    expect(html).not.toMatch(/src="data:/i);
    expect(html).not.toMatch(/onload/i);
    expect(html).toContain('data-kp-blocked="inline"');
  });

  it('相同内容命中缓存（LRU 以 content 哈希为键）', () => {
    const first = renderMarkdown('# 缓存');
    const second = renderMarkdown('# 缓存');
    expect(second).toBe(first);
    expect(renderStats().hits).toBeGreaterThanOrEqual(1);
  });

  it('不同 cacheScope 不串缓存', () => {
    renderMarkdown('![图](a.png)', { cacheScope: 'vault-a/notes', resolveAsset: (s) => `asset://a/${s}` });
    const other = renderMarkdown('![图](a.png)', {
      cacheScope: 'vault-b/notes',
      resolveAsset: (s) => `asset://b/${s}`,
    });
    expect(other).toContain('asset://b/a.png');
    expect(renderStats().misses).toBe(2);
  });

  it('LRU 超出容量时淘汰最旧的一项', () => {
    for (let i = 0; i < 25; i += 1) {
      renderMarkdown(`# ${i}`, { cacheCapacity: 20 });
    }
    expect(renderStats().size).toBe(20);
  });
});

/** 去掉块注释与行注释，**保留行号**（块注释整体替换成等量空白，换行不变）。 */
const stripComments = (text: string): string =>
  text
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .split('\n')
    .map((l) => l.split('//')[0] ?? '')
    .join('\n');

describe('R-05 / FR-EDITOR-42：禁止 setConfig', () => {
  it('src 中不存在 DOMPurify.setConfig 调用（净化配置必须逐次传参）', () => {
    const offenders: string[] = [];
    const walk = (dir: string): void => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) {
          walk(path);
        } else if (/\.(ts|vue)$/.test(name)) {
          // **逐行判定**（审计 T-01）：此前用的是整文件白名单 `!/禁止/.test(text)` ——
          // 而 sanitize.ts / renderer.ts 的注释里恰好写着"禁止 setConfig"，于是这两个
          // **唯一可能出现 setConfig 的文件**被整体豁免，R-05 守卫等于不存在。
          // 现在：注释行不算违规；确需豁免必须在**同一行**写 `// r05-ok: 理由`（强制留痕）。
          stripComments(readFileSync(path, 'utf8'))
            .split('\n')
            .forEach((line, i) => {
              const code = line.split('//')[0] ?? '';
              if (!/\.setConfig\s*\(/.test(code)) return;
              if (/r05-ok:/.test(line)) return;
              offenders.push(`${path}:${i + 1}`);
            });
        }
      }
    };
    walk('src');
    expect(offenders).toEqual([]);
  });

  it('**守卫自检**：能抓到真实调用、不误伤注释、尊重行内豁免（防止它再次静默失效）', () => {
    const scan = (text: string, path = 'x.ts'): string[] => {
      const out: string[] = [];
      stripComments(text)
        .split('\n')
        .forEach((code, i) => {
          if (!/\.setConfig\s*\(/.test(code)) return;
          if (/r05-ok:/.test(text.split('\n')[i] ?? '')) return;
          out.push(`${path}:${i + 1}`);
        });
      return out;
    };
    // ① 真实调用必须被抓到（哪怕是"含禁止二字"的文件 —— 这正是此前漏掉的情形）
    expect(scan('// 禁止使用 DOMPurify.setConfig()\nDOMPurify.setConfig({});\n')).toEqual(['x.ts:2']);
    // ② 纯注释不算违规（行注释与**块注释**都要覆盖 —— sanitize.ts:3 就是块注释里的说明）
    expect(scan('// 禁止使用 DOMPurify.setConfig()\nconst x = 1;\n')).toEqual([]);
    expect(scan('/** 禁止使用 DOMPurify.setConfig()（红线 R-05） */\nconst y = 2;\n')).toEqual([]);
    // ③ 行内豁免必须带理由标记才生效
    expect(scan('DOMPurify.setConfig({}); // r05-ok: 迁移期临时\n')).toEqual([]);
  });
});
