import { beforeEach, describe, expect, it } from 'vitest';
import { registerKpInline } from '@core/markdown/kpInline';
import { clearRenderCache, renderMarkdown } from '@core/markdown/renderer';

/**
 * 阅读态扩展语法（DEBT-34 的渲染半边；FR-EDITOR-02/25/27、FR-TAG-05）。
 *
 * **口径基准是 Rust 解析器**（`crates/kp-domain/src/md_parse/{wikilink,tag}.rs`）：
 * 渲染侧一旦宽一格，就会出现"看得见、点不了/点进去是空"的假标签或假链接。
 * 因此这里的用例逐条覆盖 MD-WL-01~09 / MD-TAG-01~06 在渲染侧的可观测后果。
 */
describe('wikilink 渲染（MD-WL-01/04/07/08；FR-EDITOR-02）', () => {
  beforeEach(() => clearRenderCache());

  it('`[[目标]]` 产出带 data-kp-link 的节点，显示文本就是目标', () => {
    const html = renderMarkdown('见 [[笔记A]]');
    expect(html).toContain('data-kp-link="笔记A"');
    expect(html).toContain('data-kp-kind="link"');
    expect(html).toContain('>笔记A<');
  });

  it('`[[目标|别名]]` 显示别名、链接目标是目标（MD-WL-04）', () => {
    const html = renderMarkdown('见 [[notes/a|点这里]]');
    expect(html).toContain('data-kp-link="notes/a"');
    expect(html).toContain('>点这里<');
    expect(html).not.toContain('>notes/a<');
  });

  it('`[[目标#锚点]]` 把锚点单列（data-kp-anchor），目标不含 `#`', () => {
    const html = renderMarkdown('见 [[笔记#标题]]');
    expect(html).toContain('data-kp-link="笔记"');
    expect(html).toContain('data-kp-anchor="标题"');
  });

  it('`![[目标]]` 标成 embed（FR-EDITOR-29 是 P1：本轮只标形态，不渲染内容）', () => {
    const html = renderMarkdown('嵌入 ![[图片.png]]');
    expect(html).toContain('data-kp-link="图片.png"');
    expect(html).toContain('data-kp-kind="embed"');
    expect(html).not.toContain('<img');
  });

  it('被转义的 `\\[[x]]` 不算链接（MD-WL-08）', () => {
    const html = renderMarkdown('转义 \\[[不是链接]]');
    expect(html).not.toContain('data-kp-link');
    expect(html).toContain('[[不是链接]]');
  });

  it('行内代码里的 `[[x]]` 不算链接（MD-WL-05）', () => {
    const html = renderMarkdown('代码 `[[x]]`');
    expect(html).not.toContain('data-kp-link');
    expect(html).toContain('<code>[[x]]</code>');
  });

  it('代码块里的 `[[x]]` 同样不算（MD-WL-05）', () => {
    const html = renderMarkdown('```\n[[x]]\n```');
    expect(html).not.toContain('data-kp-link');
  });

  it('链接体里的 `<` / `"` 不会变成 DOM 结构（用解析后的属性值断言，而不是比字面量）', () => {
    // 注意：净化器会把属性里的实体**解回**真实字符（`&lt;` → `<`）—— 这在带引号的属性里是安全的。
    // 所以判据不能是"HTML 里含不含 &lt;"，而要问 DOM：**有没有多出一个元素**、属性值是不是原文。
    const html = renderMarkdown('[[a<b]] 与 [[x"y]]');
    const doc = new DOMParser().parseFromString(html, 'text/html');
    expect(doc.querySelector('b')).toBeNull();
    const targets = Array.from(doc.querySelectorAll('[data-kp-link]')).map((el) => el.getAttribute('data-kp-link'));
    expect(targets).toEqual(['a<b', 'x"y']);
    expect(doc.querySelector('[data-kp-link]')?.textContent).toBe('a<b');
  });
});

describe('标签渲染（MD-TAG-01/02/04/05；FR-TAG-05）', () => {
  beforeEach(() => clearRenderCache());

  it('`#标签` 产出带 data-kp-tag 的节点', () => {
    const html = renderMarkdown('这是 #标签 的用法');
    expect(html).toContain('data-kp-tag="标签"');
    expect(html).toContain('>#标签<');
  });

  it('层级标签 `#a/b/c` 整体作为一个标签（不拆成三个）', () => {
    const html = renderMarkdown('层级 #项目/alpha/前端');
    expect(html).toContain('data-kp-tag="项目/alpha/前端"');
    expect((html.match(/data-kp-tag=/g) ?? []).length).toBe(1);
  });

  it('MD-TAG-01：纯数字 #123 与"词中井号" abc#def 都不算标签', () => {
    const html = renderMarkdown('数字 #123 与 abc#def');
    expect(html).not.toContain('data-kp-tag');
  });

  it('MD-TAG-04：# 后跟空格是标题，不是标签', () => {
    const html = renderMarkdown('# 标题');
    expect(html).toContain('<h1>标题</h1>');
    expect(html).not.toContain('data-kp-tag');
  });

  it('行内代码与代码块里的 `#tag` 不算标签（MD-TAG-04）', () => {
    expect(renderMarkdown('代码 `#y`')).not.toContain('data-kp-tag');
    clearRenderCache();
    expect(renderMarkdown('```\n#tag\n```')).not.toContain('data-kp-tag');
  });

  it('MD-TAG-05：保留原文大小写（#Work 与 #work 是同一个 norm，但显示各自原样）', () => {
    const html = renderMarkdown('#Work 与 #work');
    expect(html).toContain('data-kp-tag="Work"');
    expect(html).toContain('data-kp-tag="work"');
  });

  it('中文全角标点后仍识别标签（中文正文的常见写法）', () => {
    const html = renderMarkdown('中文标点后：#标签，继续');
    expect(html).toContain('data-kp-tag="标签"');
    expect(html).toContain('，继续');
  });

  it('URL 片段（#frag）不算标签', () => {
    expect(renderMarkdown('http://e.com/#frag')).not.toContain('data-kp-tag');
  });

  it('尾部斜杠按索引口径折叠：#/a//b/ ⇒ norm a/b', () => {
    const html = renderMarkdown('标签 #/a//b/ 折叠');
    expect(html).toContain('data-kp-tag="a/b"');
    expect(html).toContain('>#a/b<');
  });
});

describe('扩展语法不得破坏既有渲染（回归）', () => {
  beforeEach(() => clearRenderCache());

  it('图片仍然渲染成 <img>（历史上被自研 text 规则连累过）', () => {
    const html = renderMarkdown('![图](a.png)', { resolveAsset: (s) => `asset://v/${s}` });
    expect(html).toContain('<img');
    expect(html).toContain('src="asset://v/a.png"');
  });

  it('普通 Markdown 链接不受影响', () => {
    const html = renderMarkdown('[文字](https://e.com/a)');
    expect(html).toContain('<a href="https://e.com/a"');
    expect(html).toContain('>文字</a>');
  });

  it('加粗/行内代码/标题照旧', () => {
    const html = renderMarkdown('# 标题\n\n**粗** 与 \u0060code\u0060');
    expect(html).toContain('<h1>标题</h1>');
    expect(html).toContain('<strong>粗</strong>');
    expect(html).toContain('<code>code</code>');
  });

  it('同一段落里链接、标签、图片共存时互不吞噬', () => {
    const html = renderMarkdown('[[A]] 和 #标签 与 ![图](a.png)');
    expect(html).toContain('data-kp-link="A"');
    expect(html).toContain('data-kp-tag="标签"');
    expect(html).toContain('<img');
  });
});

describe('registerKpInline 的链上顺序（实现要点，防回归）', () => {
  it('两条规则都落在 text 之前，且 text 仍是默认实现（切分替身会连累图片）', () => {
    const calls: { name: string; before: string }[] = [];
    const fake = {
      inline: {
        ruler: {
          before: (before: string, name: string) => calls.push({ name, before }),
          at: () => {
            throw new Error('不应替换 text：默认实现本来就停在 ! [ #');
          },
        },
      },
      renderer: { rules: {} as Record<string, unknown> },
    };
    registerKpInline(fake as never);
    expect(calls.map((c) => c.name)).toEqual(['kp_link', 'kp_tag']);
    expect(calls.every((c) => c.before === 'text')).toBe(true);
  });
});
