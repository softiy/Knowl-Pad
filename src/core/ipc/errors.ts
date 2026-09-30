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
  if (isRecord(err) && typeof err.code === 'string' && typeof err.message === 'string') {
    const init: KpErrorInit = { code: err.code, message: err.message };
    if (typeof err.detail === 'string') init.detail = err.detail;
    if (isRecord(err.context)) init.context = err.context;
    return new KpError(init);
  }
  if (err instanceof Error) return new KpError({ code: 'E_INTERNAL', message: err.message });
  return new KpError({ code: 'E_INTERNAL', message: String(err) });
}
