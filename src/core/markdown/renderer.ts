import MarkdownIt from 'markdown-it';
import { highlightCodeBlock, resolveLanguage } from './highlight';
import { registerKpInline } from './kpInline';
import { sanitizeHtml } from './sanitize';

/**
 * Markdown 渲染管线（技术方案 §7.2）：
 * ① markdown-it 解析 → ② DOMPurify 净化 → ③ 远程图片策略 → ④ 交给唯一的 v-html 位置插入 DOM。
 *
 * 安全约束：
 * - `linkify` / `typographer` 一律关闭（两者是 markdown-it DoS 面的触发项，SEC-03）；
 * - 净化配置**逐次传参**，禁止 `setConfig`（R-05）；
 * - 远程图片**永不加载**，只留占位符与 URL（SEC-08 / AC-SEC-03：应用进程零网络请求）。
 */

/** 渲染可选参数。 */
export interface RenderOptions {
  /**
   * 本地附件解析（生产环境用 Tauri 的 `convertFileSrc` 转成 `asset://`）。
   * 返回 `null` 表示无法解析——此时保留原 src（仍然禁止远程加载）。
   */
  resolveAsset?: ((src: string) => string | null) | undefined;
  /** 缓存作用域：必须能区分解析上下文（如 Vault + 笔记所在目录），否则会串缓存。 */
  cacheScope?: string | undefined;
  /** LRU 容量，默认 20（§7.2 规定）。 */
  cacheCapacity?: number | undefined;
}

/** 渲染结果缓存容量（§7.2：以 content 哈希为键，LRU 20 项）。 */
export const DEFAULT_CACHE_CAPACITY = 20;

/**
 * 全项目唯一的 markdown-it 实例（html 打开、linkify/typographer 关闭 —— 见文件头）。
 *
 * **2026-10-16（M5 批次 2 的 PR-D）关闭 DEBT-34 的渲染半边**：扩展语法（`[[x]]` / `[[x|别名]]` /
 * `![[x]]` / `#标签`）由**自研 inline 规则**渲染成带 `data-kp-link` / `data-kp-tag` 的中转节点
 * （`kpInline.ts`，零新依赖）。此前这里只有一句"插件挂载于此"的注释而没有 `md.use`，
 * 阅读态把扩展语法原样当正文显示 —— 该缺陷见技术方案 §13.2 的 **DEBT-34**。
 *
 * **仍然不做**：Block ID（`^block`，只有嵌入语法用到）与嵌入渲染（`![[图片]]`/`![[笔记#标题]]`，
 * FR-EDITOR-29 是 **P1**，本轮只标 `data-kp-kind="embed"`）。 */
const md = new MarkdownIt({ html: true, linkify: false, typographer: false });
registerKpInline(md);

/**
 * 代码块语法高亮（**FR-EDITOR-44** P0 / DEBT-18）：**覆盖默认 fence 渲染器** —— 技术方案 §2.2 写明的
 * 集成方式就是「在 `md.renderer.rules.fence` 里调 `highlightCodeBlock(token.content, token.info)`」。
 *
 * 为什么覆盖渲染器而不是注册 plugin：高亮只需要「拿到代码文本 → 换成带 class 的 span」，覆盖渲染器是
 * 最小介入点；plugin 会改 token 流，把"着色"变成"解析"，HL-02（先转义再包裹）/ HL-03（未知语言降级）
 * 都更难守。
 *
 * **只处理 fence**：缩进式代码块（4 空格）走 `code_block` 规则，不在此列；**inline 代码不高亮**
 * （§2.2：内容过短、无法可靠判定语言）。
 *
 * 语言 class 只对**受支持**的语言输出（`kp-hl--js` 等），未知语言只给基类 `kp-hl`；class 的取值全部
 * 来自本模块的常量 / 固定枚举，**没有任何来自输入的插值** —— 这是 FR-EDITOR-44「不得引入执行代码的路径」
 * 在集成层的落点。
 */
md.renderer.rules.fence = (tokens, idx) => {
  const token = tokens[idx];
  const lang = resolveLanguage(token.info);
  const cls = lang === null ? 'kp-hl' : `kp-hl kp-hl--${lang}`;
  const body = highlightCodeBlock(token.content, token.info);
  return `<pre class="kp-pre"><code class="${cls}">${body}</code></pre>\n`;
};

/** 插入序即 LRU 序：命中后重新 set 使其变为最新。 */
const cache = new Map<string, string>();
let hits = 0;
let misses = 0;

/** FNV-1a 32 位（十六进制）：仅作缓存键，不需要密码学强度。 */
function contentHash(input: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

/** `http(s)://` 与协议相对 `//host/...` 都算远程。 */
function isRemoteSrc(src: string): boolean {
  return /^(?:https?:)?\/\//i.test(src);
}

/**
 * ③ 远程图片策略（SEC-08）：把 `<img>` 换成带 `data-kp-remote-src` 的占位符；
 * 本地附件补 `referrerpolicy="no-referrer"` 与 `loading="lazy"`。
 *
 * 安全性：输入已经是净化后的 HTML，这里只**新增固定属性**与**替换节点**，
 * 占位符里携带的 URL 来自通过 ALLOWED_URI_REGEXP 的 src（仅 http(s)/mailto/asset/tauri），
 * 因此不存在把 `javascript:` 之类带出去的可能。
 */
function applyImagePolicy(html: string, resolveAsset?: RenderOptions['resolveAsset']): string {
  if (!html.includes('<img')) {
    return html;
  }
  const doc = new DOMParser().parseFromString(html, 'text/html');
  for (const img of Array.from(doc.querySelectorAll('img'))) {
    const src = img.getAttribute('src') ?? '';
    // data: URI 不在允许清单内，但 DOMPurify 对 img 的 DATA_URI_TAGS **默认放行**，
    // 且 SVG 形态可携带脚本 —— 这里显式阻止：移除 src、留标记（既不加载也不丢信息）。
    if (/^data:/i.test(src)) {
      img.removeAttribute('src');
      img.setAttribute('data-kp-blocked', 'inline');
      img.setAttribute('referrerpolicy', 'no-referrer');
      img.setAttribute('loading', 'lazy');
      continue;
    }
    if (isRemoteSrc(src)) {
      const placeholder = doc.createElement('span');
      placeholder.className = 'kp-remote-image';
      placeholder.setAttribute('data-kp-remote-src', src);
      placeholder.setAttribute('role', 'link');
      placeholder.setAttribute('tabindex', '0');
      placeholder.textContent = `远程图片（点击在浏览器中打开）：${src}`;
      img.replaceWith(placeholder);
      continue;
    }
    const resolved = resolveAsset?.(src);
    if (resolved) {
      img.setAttribute('src', resolved);
    }
    img.setAttribute('referrerpolicy', 'no-referrer');
    img.setAttribute('loading', 'lazy');
  }
  return doc.body.innerHTML;
}

/** 渲染一篇 Markdown 为**可安全插入 DOM** 的 HTML（同一内容命中缓存时直接复用）。 */
export function renderMarkdown(source: string, options: RenderOptions = {}): string {
  const key = `${options.cacheScope ?? ''}\u0000${contentHash(source)}`;
  const cached = cache.get(key);
  if (cached !== undefined) {
    hits += 1;
    cache.delete(key);
    cache.set(key, cached);
    return cached;
  }
  misses += 1;
  const html = applyImagePolicy(sanitizeHtml(md.render(source)), options.resolveAsset);
  cache.set(key, html);
  const capacity = options.cacheCapacity ?? DEFAULT_CACHE_CAPACITY;
  while (cache.size > capacity) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) {
      break;
    }
    cache.delete(oldest);
  }
  return html;
}

/** 缓存统计（调试与测试用）。 */
export function renderStats(): { size: number; hits: number; misses: number } {
  return { size: cache.size, hits, misses };
}

/** 清空缓存（切换 Vault 时必须调用，避免跨库串内容）。 */
export function clearRenderCache(): void {
  cache.clear();
  hits = 0;
  misses = 0;
}
