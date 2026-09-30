import { selfTest } from '@core/markdown/sanitize';

/** 启动编排：净化器自检失败即阻断启动（AC-SEC-01）。 */
export function bootstrap(): void {
  selfTest();
}
