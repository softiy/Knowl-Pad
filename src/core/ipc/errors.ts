/**
 * 搜索语法错误码（PRD §5.2 第 1561 行：搜索语法错误 / 400 / **显示语法提示**；
 * FR-SEARCH-27 与 AC-SEARCH-05）。
 *
 * Rust 侧由 `kp_domain::error::AppError::SearchSyntax` 映射而来：用户输入未闭合引号、
 * 未闭合的 `line:(`、只有键没有值的过滤条件时返回它。前端**必须**据它显示语法提示，
 * 而不是把空结果当成「没有匹配」—— AC-SEARCH-05 明令不得静默返回空结果。
 */
export const E_SEARCH_SYNTAX = 'E_SEARCH_SYNTAX';

/**
 * 搜索"范围太大"错误码（PRD §5.2，2026-10-15 裁定①新增）。
 *
 * 为什么单独一个码：候选数超限（如 `line:` 的候选超过上限）**语法没写错**，只是范围太大。
 * 此前它与语法错误共用 {@link E_SEARCH_SYNTAX}，前端只能按 message 子串猜 —— 后端一改文案就静默
 * 退化成"语法提示"。现在两者在**码层面**分开，前端主路径只看码。
 */
export const E_SEARCH_TOO_BROAD = 'E_SEARCH_TOO_BROAD';

/** 是否搜索语法错误（把错误码收敛到一处，调用点不再手写字符串）。 */
export function isSearchSyntaxError(err: unknown): boolean {
  return asKpError(err).code === E_SEARCH_SYNTAX;
}

/** 是否"搜索范围太大"（与语法错误分开的专用码；见 {@link E_SEARCH_TOO_BROAD}）。 */
export function isSearchTooBroadError(err: unknown): boolean {
  return asKpError(err).code === E_SEARCH_TOO_BROAD;
}

/**
 * 改写预览已过期 / 已被消费（PRD §5.2：410 / **提示重新执行预览**）。
 *
 * 只对 `*_preview` → `*_apply` 的两步流程有意义（链接域与标签域共用）：
 * `preview_id` 是一次性消费的，第二次点"确认执行"必然撞上它。
 * 前端**必须**据此提示"预览已过期，请重新预览"，而不是把失败当成"改写失败、磁盘可能已改坏"。
 */
export const E_PREVIEW_EXPIRED = 'E_PREVIEW_EXPIRED';

/**
 * 名称非法（PRD §5.2：400 / 提示具体原因）。
 *
 * 标签域用它承载 FR-TAG-08 的层级冲突：`to_tag` 空、首尾或连续斜杠、含空白、
 * 新名是自身的子孙（循环）都归这里 —— 后端拒绝，磁盘零改动。
 */
export const E_INVALID_FILENAME = 'E_INVALID_FILENAME';

/** 是否预览过期（把错误码收敛到一处，调用点不再手写字符串）。 */
export function isPreviewExpired(err: unknown): boolean {
  return asKpError(err).code === E_PREVIEW_EXPIRED;
}

/** 是否名称非法（标签重命名里的层级冲突见 {@link E_INVALID_FILENAME}）。 */
export function isInvalidNameError(err: unknown): boolean {
  return asKpError(err).code === E_INVALID_FILENAME;
}

/**
 * 该错误是否**真的带了 IPC 错误码**（而不是被 {@link asKpError} 兜底合成出来的 `E_INTERNAL`）。
 *
 * 用途只有一个：让"按 message 子串猜"的旧逻辑退化成**显式兜底**（仅当响应无码时才启用），
 * 而不是与码并行的第二条真相。
 */
export function hasWireCode(err: unknown): boolean {
  if (err instanceof KpError) return true;
  return isRecord(err) && typeof err.code === 'string' && typeof err.message === 'string';
}

export interface KpErrorInit {
  code: string;
  message: string;
  detail?: string;
  context?: Record<string, unknown>;
}

/** IPC 错误：继承 Error，便于 throw 与堆栈追踪，同时携带 PRD §5.2 的错误码。 */
export class KpError extends Error {
  readonly code: string;
  readonly detail?: string;
  readonly context?: Record<string, unknown>;

  constructor(init: KpErrorInit) {
    super(init.message);
    this.name = 'KpError';
    this.code = init.code;
    if (init.detail !== undefined) this.detail = init.detail;
    if (init.context !== undefined) this.context = init.context;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** 把 IPC 抛出的任意值收敛为 KpError（Rust 侧 AppError 已序列化为该结构）。 */
export function asKpError(err: unknown): KpError {
  // 已经是 KpError 就原样返回（幂等）。否则会**再包一层**、丢掉原始堆栈 ——
  // 审计 T-31 由新加的 ipc-layer 用例抓出来的真实缺陷。
  if (err instanceof KpError) return err;
  if (isRecord(err) && typeof err.code === 'string' && typeof err.message === 'string') {
    const init: KpErrorInit = { code: err.code, message: err.message };
    if (typeof err.detail === 'string') init.detail = err.detail;
    if (isRecord(err.context)) init.context = err.context;
    return new KpError(init);
  }
  if (err instanceof Error) return new KpError({ code: 'E_INTERNAL', message: err.message });
  return new KpError({ code: 'E_INTERNAL', message: String(err) });
}
