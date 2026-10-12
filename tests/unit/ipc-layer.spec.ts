//! IPC 层的**真实现**用例（审计 T-31）。
//!
//! 此前 src/core/ipc/** 被覆盖率 exclude 掉、且被各 feature 测试全量 mock，
//! 于是「全项目唯一的 invoke 出口」与 asKpError **从未真正执行过**：
//! 错误码映射写错、args 没透传、detail/context 丢字段，都不会有任何用例报警。
//!
//! 这里只 mock Tauri 的原语 invoke（进程边界必须 mock），其余走真实现。

import { beforeEach, describe, expect, it, vi } from 'vitest';

const hoisted = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: hoisted.invoke }));

import { call, callVoid } from '@core/ipc/client';
import { KpError, asKpError } from '@core/ipc/errors';

describe('IPC 出口 call/callVoid（PRD FE-04：全项目唯一 invoke 出口）', () => {
  beforeEach(() => {
    hoisted.invoke.mockReset();
  });

  it('成功时把命令名与参数原样透传给 invoke，并返回其结果', async () => {
    hoisted.invoke.mockResolvedValue({ relPath: 'a.md' });
    const out = await call<{ relPath: string }>('note_read', { relPath: 'a.md' });
    expect(hoisted.invoke).toHaveBeenCalledWith('note_read', { relPath: 'a.md' });
    expect(out).toEqual({ relPath: 'a.md' });
  });

  it('不带参数时 args 保持 undefined（不能悄悄变成空对象）', async () => {
    hoisted.invoke.mockResolvedValue(null);
    await call('vault_close');
    expect(hoisted.invoke).toHaveBeenCalledWith('vault_close', undefined);
  });

  it('Rust 侧序列化的错误结构 → 抛真正的 KpError（码/文案/detail/context 逐字段保留）', async () => {
    hoisted.invoke.mockRejectedValue({
      code: 'E_FILE_LOCKED',
      message: '文件被外部程序占用：a.md',
      detail: 'os error 32',
      context: { relPath: 'a.md' },
    });
    await expect(call('note_write', { relPath: 'a.md' })).rejects.toBeInstanceOf(KpError);
    try {
      await call('note_write', { relPath: 'a.md' });
    } catch (err) {
      const e = err as KpError;
      expect(e.code).toBe('E_FILE_LOCKED');
      expect(e.message).toBe('文件被外部程序占用：a.md');
      expect(e.detail).toBe('os error 32');
      expect(e.context).toEqual({ relPath: 'a.md' });
      expect(e.name).toBe('KpError');
    }
  });

  it('普通 Error → 收敛为 E_INTERNAL，且保留原始文案（不吞消息）', async () => {
    hoisted.invoke.mockRejectedValue(new Error('invoke 崩了'));
    await expect(call('x')).rejects.toMatchObject({ code: 'E_INTERNAL', message: 'invoke 崩了' });
  });

  it('非 Error 的抛出值 → E_INTERNAL + String(err)', async () => {
    hoisted.invoke.mockRejectedValue('字符串错误');
    await expect(call('x')).rejects.toMatchObject({ code: 'E_INTERNAL', message: '字符串错误' });
  });

  it('callVoid 成功时 resolve（返回值不泄漏给调用方）', async () => {
    hoisted.invoke.mockResolvedValue(null);
    await expect(callVoid('vault_close')).resolves.toBeUndefined();
  });
});

describe('asKpError 的字段契约', () => {
  it('detail/context 缺省时不写入（保持不存在，而不是值为 undefined）', () => {
    const e = asKpError({ code: 'E_X', message: '文案' });
    expect(e.code).toBe('E_X');
    // 注意：类字段声明在 ES2022 语义下**总会定义**该属性（值为 undefined），
    // 所以这里只能断言"值未定义"，不能断言"属性不存在" —— 我第一版就写错了。
    expect(e.detail).toBeUndefined();
    expect(e.context).toBeUndefined();
  });

  it('detail 不是字符串、context 不是对象时忽略它们（不把脏数据带进错误）', () => {
    const e = asKpError({ code: 'E_X', message: '文案', detail: 42, context: 'nope' });
    // 注意：类字段声明在 ES2022 语义下**总会定义**该属性（值为 undefined），
    // 所以这里只能断言"值未定义"，不能断言"属性不存在" —— 我第一版就写错了。
    expect(e.detail).toBeUndefined();
    expect(e.context).toBeUndefined();
  });

  it('已经是 KpError 时原样返回（幂等，避免二次包装丢栈）', () => {
    const first = new KpError({ code: 'E_X', message: '文案' });
    expect(asKpError(first)).toBe(first);
  });
});

describe('命令包装层：每个包装函数都必须调用正确的命令名（IPC 契约）', () => {
  it('逐个调用 commands.ts 的全部导出，命令名 = 函数名的 snake_case', async () => {
    const mod = await import('@core/ipc/commands');
    // 已知例外（函数名与命令名不同名时在此登记，登记即留痕）
    const exceptions: Record<string, string> = {};
    const mismatches: string[] = [];
    let checked = 0;
    for (const [name, fn] of Object.entries(mod)) {
      if (typeof fn !== 'function') continue;
      const expected = exceptions[name] ?? name.replace(/[A-Z]/g, (c) => '_' + c.toLowerCase());
      hoisted.invoke.mockReset();
      hoisted.invoke.mockResolvedValue(null);
      try {
        await (fn as (...a: unknown[]) => unknown)('x', 'y', 'z');
      } catch (err) {
        if (err instanceof KpError) throw err;
      }
      checked += 1;
      const called = hoisted.invoke.mock.calls[0]?.[0];
      if (called !== expected) mismatches.push(name + ' → ' + String(called) + '（期望 ' + expected + '）');
    }
    expect(checked).toBeGreaterThan(20); // 防止扫了个空集合
    expect(mismatches).toEqual([]);
  });
});
