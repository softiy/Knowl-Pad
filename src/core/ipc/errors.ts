/**
 * 搜索语法错误码（PRD §5.2 第 1561 行：搜索语法错误 / 400 / **显示语法提示**；
 * FR-SEARCH-27 与 AC-SEARCH-05）。
 *
 * Rust 侧由 `kp_domain::error::AppError::SearchSyntax` 映射而来：用户输入未闭合引号、
 * 未闭合的 `line:(`、只有键没有值的过滤条件时返回它。前端**必须**据它显示语法提示，
 * 而不是把空结果当成「没有匹配」—— AC-SEARCH-05 明令不得静默返回空结果。
 */
export const E_SEARCH_SYNTAX = 'E_SEARCH_SYNTAX';

/** 是否搜索语法错误（把错误码收敛到一处，调用点不再手写字符串）。 */
export function isSearchSyntaxError(err: unknown): boolean {
  return asKpError(err).code === E_SEARCH_SYNTAX;
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
