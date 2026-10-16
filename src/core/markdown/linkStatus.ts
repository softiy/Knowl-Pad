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
 */
import { linkOutgoing, type OutgoingLink } from '@core/ipc/linkCommands';

/** 链接状态（在 DOM 上体现为 `kp-link--<state>`）。 */
export type KpLinkState = 'resolved' | 'dangling' | 'ambiguous';

/** 目标 → 状态的索引（键已规范化，见 {@link normalizeTarget}）。 */
export interface LinkStatusIndex {
  byTarget: Map<string, KpLinkState>;
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
 */
export function buildLinkStatusIndex(links: OutgoingLink[]): LinkStatusIndex {
  const byTarget = new Map<string, KpLinkState>();
  for (const link of links) {
    const key = normalizeTarget(link.targetRef);
    if (!key) continue;
    const next = stateOf(link);
    const prev = byTarget.get(key);
    if (prev === undefined || SEVERITY[next] < SEVERITY[prev]) byTarget.set(key, next);
  }
  return { byTarget };
}

/** 查一个链接目标的状态；清单里没有 ⇒ null（中性样式，见文件头）。 */
export function stateOfTarget(index: LinkStatusIndex, target: string): KpLinkState | null {
  return index.byTarget.get(normalizeTarget(target)) ?? null;
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
