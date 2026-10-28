/**
 * 阅读态的**点击路由判定**（FR-EDITOR-25 / FR-EDITOR-27 / FR-TAG-05）—— 纯函数，单独成文件有两个理由：
 *
 * ① `EditorView.vue` 本来只有 216 SLOC 上下，把判定塞进组件会**顶到 CODE-11 的 200 预警线**（实测 216）；
 * ② 这里的分支正是最容易错的地方（"普通点击到底该不该跳"），单独放才能逐条钉住。
 *
 * 三条口径来自需求原文：
 * - `Ctrl/Cmd+点击` → **跳转**（FR-EDITOR-25 原话："`Ctrl/Cmd+点击` wikilink 跳转到目标笔记"）；
 * - 普通点击**已裁决**链接 → **不动作**（阅读时点一下就跳走会很难用，且需求没要求单键跳转）；
 * - 普通点击**悬空**链接 → **创建**（FR-EDITOR-27："悬空链接……点击触发创建"）。
 */

/** 一次点击的意图（由组件把 DOM/事件收敛成它）。 */
export type ClickIntent =
  | { kind: 'link'; target: string }
  | { kind: 'link-new-tab'; target: string }
  | { kind: 'create'; name: string }
  | { kind: 'none' };

/** 带修饰键的跳转（NFR-PLAT-07：macOS 认 Cmd、其余认 Ctrl）。 */
export function hasJumpModifier(event: { ctrlKey?: boolean; metaKey?: boolean }): boolean {
  return event.ctrlKey === true || event.metaKey === true;
}

/**
 * 悬空链接的目标 → **新建笔记的建议名**：**只取最后一段**并在缺后缀时补 `.md`。
 *
 * 为什么只取最后一段：正文里的 `[[notes/缺笔记]]` 表示"找一篇叫 `缺笔记` 的笔记"，
 * 并不表示"必须建在 `notes/` 下"（该目录可能不存在）；把整条路径当名字会建出嵌套目录。
 * 用户仍可在对话框里改名 —— 这里给的是**建议**，不是决定。
 */
export function suggestNoteName(target: string): string {
  const last = target.split('/').filter((seg) => seg !== '').pop() ?? '';
  if (!last) return '';
  return /\.md$/i.test(last) ? last : `${last}.md`;
}

/**
 * 判定一次点击要做什么。
 *
 * `dangling` 由上层（`MarkdownView` 的链接状态标记）给出：**只有已知悬空**才会创建 ——
 * 索引还没跑完时链接是中性态，那时点它既不该跳也不该建（否则会给用户凭空造出重复笔记）。
 */
export function clickIntent(
  payload: { target: string; dangling: boolean },
  event: { ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean },
): ClickIntent {
  if (!payload.target) return { kind: 'none' };
  if (payload.dangling && !hasJumpModifier(event)) {
    const name = suggestNoteName(payload.target);
    return name ? { kind: 'create', name } : { kind: 'none' };
  }
  if (!hasJumpModifier(event)) return { kind: 'none' };
  // FR-EDITOR-25：Ctrl/Cmd+Alt+点击 ⇒ 在新标签页打开（与普通跳转区分）
  return event.altKey === true ? { kind: 'link-new-tab', target: payload.target } : { kind: 'link', target: payload.target };
}
