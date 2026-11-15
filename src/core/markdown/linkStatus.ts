/**
 * 阅读态链接的**状态解析**（FR-EDITOR-27：悬空链接在视觉上与正常链接区分）。
 *
 * 为什么不在渲染器里判断：渲染器是**纯函数 + LRU 缓存**，把索引查询塞进去会让缓存键得带上
 * "当前索引状态"（索引一变整片缓存失效）。这里改成**渲染后打标**：
 * 一次 `link_outgoing`（该笔记的全部出链 + 已有裁决结果）→ 把状态映射成 CSS 类打到已渲染的 DOM 上。
 *
 * 状态来自索引阶段的链接裁决（MD-WL-03），本模块**不重新实现裁决**：
 * - `resolved`：目标已裁决到某个 `.md` ⇒ 可跳转；
 * - `dangling`：无候选 ⇒ 视觉区分 + 点击创建（FR-EDITOR-27）；
 * - `ambiguous`：多个候选 ⇒ 视觉上也按"未确定"处理（不猜目标，避免跳错笔记）；
 * - **未知**（不在出链清单里）：保持中性样式 —— 索引还没跑完时不能把全部链接画成悬空。
 *
 * **点击跳转需要真实 relPath**（M5 真机 D2 / 10.7）：正文里的目标是**显示口径**
 * （`[[B]]` / `[[B|别名]]` 的 data-kp-link 都是 `B`），而 `note_read` 只认识
 * vault 相对路径。把 `dstRelPath` 一起带在索引里，点击时替换成它即可 —— 显示文本不变。
 */
import { linkOutgoing, type OutgoingLink } from '@core/ipc/linkCommands';

/** 链接状态（在 DOM 上体现为 `kp-link--<state>`）。 */
export type KpLinkState = 'resolved' | 'dangling' | 'ambiguous';

/** 一个目标在索引里的完整裁决结果。 */
export interface LinkTargetStatus {
  state: KpLinkState;
  /**
   * `resolved` 时指向的**真实** relPath（点击跳转用它）；其余状态恒 `null`。
   *
   * 语义上它和 `state` 是绑定的：没有目标路径的"已裁决"就是不可跳转的（见 {@link stateOf}），
   * 因此这里不会出现 `state === 'dangling' && dstRelPath !== null` 的组合。
   */
  dstRelPath: string | null;
}

/** 目标 → 裁决结果的索引（键已规范化，见 {@link normalizeTarget}）。 */
export interface LinkStatusIndex {
  byTarget: Map<string, LinkTargetStatus>;
}

/**
 * 目标规范化（**只用于比较**，不回写显示）：
 * 大小写不敏感、去 `.md` 后缀、统一分隔符、去掉前后空白。
 *
 * 为什么连大小写也折叠：Wikilink 的目标匹配本身就是"大小写不敏感"（与标签的 `norm` 同理），
 * 这一步只是把两侧拉到同一口径，**不会**改变渲染出来的字形。
 */
export function normalizeTarget(raw: string): string {
  return raw.trim().replace(/\\/g, '/').toLowerCase().replace(/\.md$/, '');
}

/** 一个出链对某个链接目标的"贡献"（多条同目标时取**最不确定**的那个，见 {@link buildLinkStatusIndex}）。 */
function stateOf(link: OutgoingLink): KpLinkState {
  if (link.status === 'resolved' && link.dstRelPath) return 'resolved';
  if (link.status === 'ambiguous') return 'ambiguous';
  return 'dangling';
}

/** 同一目标出现多条时的合并口径：歧义 > 悬空 > 已裁决（宁可提示"不确定"，也不猜一个目标跳过去）。 */
const SEVERITY: Record<KpLinkState, number> = { ambiguous: 0, dangling: 1, resolved: 2 };

/**
 * 由出链清单构建状态索引。
 *
 * `link_outgoing` 的 `targetRef` 与正文里写的目标**通常是同一个串**（裁决器用它去匹配文件名），
 * 但为了容忍 `.md` 后缀与大小写差异，这里按 {@link normalizeTarget} 折叠后建索引。
 *
 * 合并时 `dstRelPath` 只在最终状态是 `resolved` 时保留 —— 同目标出现歧义/悬空时宁可"不可跳"，
 * 也不拿其中一条的路径去跳（那正是 FR-EDITOR-27 要避免的"跳错笔记"）。
 */
export function buildLinkStatusIndex(links: OutgoingLink[]): LinkStatusIndex {
  const byTarget = new Map<string, LinkTargetStatus>();
  for (const link of links) {
    const key = normalizeTarget(link.targetRef);
    if (!key) continue;
    const state = stateOf(link);
    const dstRelPath = state === 'resolved' ? (link.dstRelPath ?? null) : null;
    const prev = byTarget.get(key);
    if (prev === undefined) {
      byTarget.set(key, { state, dstRelPath });
      continue;
    }
    // 否则保留（更确定的）已有结果。注意：`resolved` 状态的 dstRelPath 必非空
    // （`stateOf` 已把"resolved 但没路径"降级成 dangling），因此这里不需要"补路径"分支。
    if (SEVERITY[state] < SEVERITY[prev.state]) {
      byTarget.set(key, { state, dstRelPath });
    }
  }
  return { byTarget };
}

/** 查一个链接目标的完整裁决结果；清单里没有 ⇒ null（中性样式，见文件头）。 */
export function targetStatus(index: LinkStatusIndex, target: string): LinkTargetStatus | null {
  return index.byTarget.get(normalizeTarget(target)) ?? null;
}

/** 查一个链接目标的状态；清单里没有 ⇒ null（中性样式，见文件头）。 */
export function stateOfTarget(index: LinkStatusIndex, target: string): KpLinkState | null {
  return targetStatus(index, target)?.state ?? null;
}

/**
 * 查一个链接目标的**真实 relPath**（D2/10.7 的跳转目标）；未裁决/歧义/悬空/未知 ⇒ null。
 *
 * 调用方（阅读态点击）必须用 `null` 判断"能不能跳"，而不是拿显示目标兜底 ——
 * 把显示名当路径交给 `note_read` 会报「文件不存在」，正是真机缺陷 D2 的成因。
 */
export function dstRelPathOf(index: LinkStatusIndex, target: string): string | null {
  return targetStatus(index, target)?.dstRelPath ?? null;
}

/** 由索引库取该笔记的出链并建索引；失败时返回空索引（**不抛** —— 阅读不该因索引失败而空白）。 */
export async function loadLinkStatus(relPath: string): Promise<LinkStatusIndex> {
  try {
    return buildLinkStatusIndex(await linkOutgoing(relPath));
  } catch {
    return { byTarget: new Map() };
  }
}

/** 状态 → CSS 类（`resolved` 不需要额外类：默认样式即"正常链接"）。 */
export function stateClass(state: KpLinkState | null): string {
  return state === null || state === 'resolved' ? '' : `kp-link--${state}`;
}

/**
 * 把状态打到**已渲染**的 DOM 上（只改类名，不改结构）。
 *
 * 每次整片重打（先清掉三种状态类）—— 索引刷新后旧状态必须能**退掉**，
 * 否则一个刚被创建的笔记会永远显示成悬空。
 */
export function applyLinkStatus(root: ParentNode, index: LinkStatusIndex): void {
  for (const el of Array.from(root.querySelectorAll<HTMLElement>('[data-kp-link]'))) {
    const state = stateOfTarget(index, el.getAttribute('data-kp-link') ?? '');
    el.classList.remove('kp-link--dangling', 'kp-link--ambiguous');
    const cls = stateClass(state);
    if (cls) el.classList.add(cls);
  }
}
