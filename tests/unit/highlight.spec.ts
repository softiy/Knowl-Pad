/** 渲染态代码块高亮（FR-EDITOR-44 / DEBT-18）：分词单测 + 安全断言 + 性能量级。 */
import { describe, expect, it } from 'vitest';
import {
  HIGHLIGHT_LANGUAGES,
  HIGHLIGHT_LINE_LIMIT,
  highlightCodeBlock,
  isSupportedLanguage,
  resolveLanguage,
} from '@core/markdown/highlight';

/** 取某 class 的片段数量（只数开标签，便于断言「有/没有 span」）。 */
function countSpans(html: string, cls: string): number {
  return html.split('<span class="' + cls + '">').length - 1;
}

/** 用 DOMParser 判「是文本还是结构」：唯一可靠的判据（字符串里含有标签不算，要真解析）。 */
function parse(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html');
}

describe('resolveLanguage：语言判定（§2.2 的 info string 口径）', () => {
  it('规范名与常见别名都能命中', () => {
    expect(resolveLanguage('js')).toBe('js');
    expect(resolveLanguage('javascript')).toBe('js');
    expect(resolveLanguage('TS')).toBe('ts');
    expect(resolveLanguage('rust')).toBe('rust');
    expect(resolveLanguage('sh')).toBe('bash');
    expect(resolveLanguage('md')).toBe('markdown');
  });

  it('info 带额外参数时取首段（§2.2 的 fence info string）', () => {
    expect(resolveLanguage('js title="x"')).toBe('js');
    expect(resolveLanguage('  rust  ignore')).toBe('rust');
  });

  it('未知语言 / 空 info 返回 null（HL-03 的降级依据）', () => {
    expect(resolveLanguage('')).toBeNull();
    expect(resolveLanguage('foobar')).toBeNull();
    expect(isSupportedLanguage('foobar')).toBe(false);
    expect(HIGHLIGHT_LANGUAGES).toContain('rust');
  });
});

describe('highlightCodeBlock：分词（js / ts / rust / json / bash / markdown）', () => {
  it('js：关键字、字符串、注释、数字各自成 span，且内容逐字不变', () => {
    const code = "const n = 42; // 注释\nconst s = \"hi\";";
    const html = highlightCodeBlock(code, 'js');
    expect(countSpans(html, 'kp-hl-keyword')).toBe(2);
    expect(countSpans(html, 'kp-hl-number')).toBe(1);
    expect(countSpans(html, 'kp-hl-string')).toBe(1);
    expect(countSpans(html, 'kp-hl-comment')).toBe(1);
    expect(parse(html).body.textContent).toBe(code);
  });

  it('ts：类型关键字也按关键字着色（关键字集合比 js 宽）', () => {
    const html = highlightCodeBlock("interface A { x: number }", 'ts');
    expect(countSpans(html, 'kp-hl-keyword')).toBe(1);
    expect(parse(html).body.textContent).toBe("interface A { x: number }");
  });

  it('rust：字符字面量着色，生命周期不着色', () => {
    const lifetime = "fn f<'a>(x: &'a str) -> &'a str { x }";
    expect(countSpans(highlightCodeBlock(lifetime, 'rust'), 'kp-hl-string')).toBe(0);
    const withChar = "let c = 'x';";
    expect(countSpans(highlightCodeBlock(withChar, 'rust'), 'kp-hl-string')).toBe(1);
  });

  it('rust：原始字符串 r#... 整段是字符串', () => {
    const raw = "let s = r#\"a \"b\" c\"#;";
    const html = highlightCodeBlock(raw, 'rust');
    expect(countSpans(html, 'kp-hl-string')).toBe(1);
    expect(parse(html).body.textContent).toBe(raw);
  });

  it('json：true/false/null 是字面量，字符串是字符串', () => {
    const html = highlightCodeBlock('{"a": true, "b": null}', 'json');
    expect(countSpans(html, 'kp-hl-literal')).toBe(2);
    expect(countSpans(html, 'kp-hl-string')).toBe(2);
  });

  it('bash：$VAR 是变量，行首 # 是注释（a#b 里的 # 不是）', () => {
    const html = highlightCodeBlock("# 注释\necho $HOME a#b", 'bash');
    expect(countSpans(html, 'kp-hl-comment')).toBe(1);
    expect(countSpans(html, 'kp-hl-var')).toBe(1);
  });

  it('markdown：标题/引用/列表/行内代码/链接各有着色', () => {
    const code = "# Title\n> quote\n- item `x` [t](u)";
    const html = highlightCodeBlock(code, 'markdown');
    expect(countSpans(html, 'kp-hl-heading')).toBe(1);
    expect(countSpans(html, 'kp-hl-quote')).toBe(1);
    expect(countSpans(html, 'kp-hl-list')).toBe(1);
    expect(countSpans(html, 'kp-hl-code')).toBe(1);
    expect(countSpans(html, 'kp-hl-link')).toBe(1);
    expect(countSpans(html, 'kp-hl-url')).toBe(1);
  });

  it('未闭合字符串吃到行尾，不吃掉后面的行', () => {
    const code = "const s = \"abc\nconst t = 1;";
    const html = highlightCodeBlock(code, 'js');
    expect(countSpans(html, 'kp-hl-number')).toBe(1);
  });
});

describe('HL-02 / FR-EDITOR-44：高亮只产出文本与 span（无执行路径）', () => {
  it('代码里出现 <script> / onerror / 引号时，全部是文本节点', () => {
    const code = "const s = \"<script>alert(1)</script>\"; // onerror=alert(2)";
    const html = highlightCodeBlock(code, 'js');
    const doc = parse(html);
    expect(doc.querySelector('script')).toBeNull();
    expect(doc.body.textContent).toContain('<script>alert(1)</script>');
    expect(doc.body.textContent).toContain('onerror=alert(2)');
    expect(doc.body.querySelectorAll('*[onerror], *[onload], *[onclick]').length).toBe(0);
  });

  it('HTML 实体在文本里保持字面（& < > 引号全部转义）', () => {
    const html = highlightCodeBlock("a = 1 < 2 && \"x\"", 'js');
    expect(html).toContain('&lt;');
    expect(html).toContain('&amp;&amp;');
    expect(html).toContain('&quot;x&quot;');
  });

  it('HL-03：未知语言与无语言都降级为纯文本（不报错、不吞内容）', () => {
    const code = "a<b && c\n第二行";
    for (const info of ['', 'foobar', 'python']) {
      const html = highlightCodeBlock(code, info);
      expect(html).not.toContain('<span');
      expect(html).toContain('&lt;');
      expect(parse(html).body.textContent).toBe(code);
    }
  });

  it('HL-07：超过行数上限的块跳过高亮（内容仍旧完整可见）', () => {
    const many = "let a = 1;\n".repeat(HIGHLIGHT_LINE_LIMIT + 1);
    const html = highlightCodeBlock(many, 'js');
    expect(html).not.toContain('<span');
    expect(parse(html).body.textContent).toBe(many);
  });
});

describe('HL-07：大代码块的耗时量级（同步高亮不得阻塞主线程）', () => {
  it('2000 行 js 块 < 200ms（量级断言；机器前提：本机 24 核、无调试器；CI 共享 runner 可能更慢）', () => {
    const code = "const value = 123; // 注释\n".repeat(HIGHLIGHT_LINE_LIMIT);
    const started = performance.now();
    const html = highlightCodeBlock(code, 'js');
    const elapsed = performance.now() - started;
    // 把真实数字打进测试输出（量级断言失败时也能直接看到差多少）
    console.log('HL-07 perf: 2000 行 js 高亮耗时 ' + elapsed.toFixed(1) + ' ms');
    expect(html).toContain('<span');
    expect(elapsed).toBeLessThan(200);
  });
});