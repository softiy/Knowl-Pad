/** 文件树纯逻辑（FR-FILE-02 排序 / FR-FILE-07 Wikilink / 5.3–5.6 的回归口径；DEBT-19）。 */
import { describe, expect, it } from 'vitest';
import type { FileNode } from '@core/ipc/commands';
import {
  compareNames,
  defaultDirectionOf,
  ensureNoteExtension,
  extensionOf,
  isSameRelPath,
  normalizeRelPath,
  sortNodes,
  wikilinkOf,
} from '@features/file-tree/logic';

function node(relPath: string, isDir = false, mtimeMs?: number): FileNode {
  const name = relPath.split('/').pop() ?? relPath;
  const base: FileNode = { relPath, name, isDir, kind: isDir ? 'other' : 'note' };
  // exactOptionalPropertyTypes：未给 mtimeMs 时必须**省略**该键，不能写成 undefined
  return mtimeMs === undefined ? base : { ...base, mtimeMs };
}

describe('sortNodes：FR-FILE-02 的三种排序', () => {
  it('三种模式下文件夹都优先（PRD 原文的硬性要求）', () => {
    const nodes = [node('a.md', false, 900), node('zdir', true, 100), node('b.md', false, 500)];
    for (const mode of ['name', 'mtime', 'type'] as const) {
      const sorted = sortNodes(nodes, mode).map((n) => n.name);
      expect(sorted[0]).toBe('zdir');
    }
  });

  it('name：按名称（zh-CN + numeric，a2 在 a10 前）', () => {
    const nodes = [node('a10.md'), node('a2.md'), node('b.md')];
    expect(sortNodes(nodes, 'name').map((n) => n.name)).toEqual(['a2.md', 'a10.md', 'b.md']);
  });

  it('mtime：新的在前，缺 mtime 的按名称退让', () => {
    const nodes = [node('old.md', false, 100), node('new.md', false, 900), node('none.md')];
    expect(sortNodes(nodes, 'mtime').map((n) => n.name)).toEqual(['new.md', 'old.md', 'none.md']);
  });

  it('type：按扩展名分组，同组按名称，无扩展名排最后', () => {
    const nodes = [node('z.png'), node('a.md'), node('b.md'), node('noext')];
    expect(sortNodes(nodes, 'type').map((n) => n.name)).toEqual(['a.md', 'b.md', 'z.png', 'noext']);
  });

  it('不修改入参（返回新数组）', () => {
    const nodes = [node('b.md'), node('a.md')];
    const sorted = sortNodes(nodes, 'name');
    expect(nodes.map((n) => n.name)).toEqual(['b.md', 'a.md']);
    expect(sorted.map((n) => n.name)).toEqual(['a.md', 'b.md']);
  });
});

describe('sortNodes：并列与方向（M5 冒烟 5.3 的根因与口径）', () => {
  it('mtime 并列（同一毫秒）时按名称自然序兜底，且结果与输入顺序无关', () => {
    // 5.3 的现场：a10 与 b.txt 的 mtimeMs **完全相同**（Windows 文件时间在同一时钟节拍内会相等，
    // Rust 侧再按毫秒截断），于是"谁先谁后"不可能从 mtime 分辨 —— 旧实现给出 a10 在前，
    // 用户按创建顺序期望 b.txt 在前。并列下的唯一可解释规则是名称自然序，并必须**确定性**。
    const tied = [node('b.txt', false, 1_000), node('a10', false, 1_000)];
    expect(sortNodes(tied, 'mtime').map((n) => n.name)).toEqual(['a10', 'b.txt']);
    expect(sortNodes([...tied].reverse(), 'mtime').map((n) => n.name)).toEqual(['a10', 'b.txt']);
  });

  it('5.3 清单回归：时间最近优先；并列项按名称自然序（a2 在 a10 前、a10 在 b.txt 前）', () => {
    const files = [
      node('b.txt', false, 5_000),
      node('a10', false, 5_000), // 与 b.txt 同一毫秒 ⇒ 并列
      node('z', false, 4_000),
      node('C.MD', false, 3_000),
      node('a2', false, 2_000),
      node('notes', false, 1_000),
    ];
    // 期望/实际首两位互换的解释：并列项按名称自然序 ⇒ a10 在前是**确定性规则**，
    // 创建顺序（b.txt 在前）在相同 mtime 下不可恢复。
    expect(sortNodes(files, 'mtime').map((n) => n.name)).toEqual([
      'a10',
      'b.txt',
      'z',
      'C.MD',
      'a2',
      'notes',
    ]);
  });

  it('mtime 方向可翻转：升序 = 最旧优先，并列仍按名称升序兜底', () => {
    const files = [node('b.txt', false, 5_000), node('a10', false, 5_000), node('z', false, 1_000)];
    expect(sortNodes(files, 'mtime', 'asc').map((n) => n.name)).toEqual(['z', 'a10', 'b.txt']);
  });

  it('name/type 的默认方向是升序，可显式翻转为降序', () => {
    const files = [node('a.md'), node('b.png'), node('c.md')];
    expect(sortNodes(files, 'name', 'desc').map((n) => n.name)).toEqual(['c.md', 'b.png', 'a.md']);
    // type：只翻转扩展名分组，组内仍按名称升序（md 组 a,c；png 组 b）
    expect(sortNodes(files, 'type', 'desc').map((n) => n.name)).toEqual(['b.png', 'a.md', 'c.md']);
  });

  it('defaultDirectionOf：时间降序（最近优先），名称/类型升序', () => {
    expect(defaultDirectionOf('mtime')).toBe('desc');
    expect(defaultDirectionOf('name')).toBe('asc');
    expect(defaultDirectionOf('type')).toBe('asc');
  });

  it('compareNames 是全序：大小写不同的同名文件也有确定顺序（不依赖输入顺序）', () => {
    expect(compareNames('A.md', 'a.md')).toBeLessThan(0);
    expect(compareNames('a.md', 'A.md')).toBeGreaterThan(0);
    expect(compareNames('a2', 'a10')).toBeLessThan(0);
  });
});

describe('路径归一与「与当前相同」判定（M5 冒烟 5.4）', () => {
  it('normalizeRelPath：去空白、统一分隔符、折叠重复分隔符、去 ./ 与结尾 /', () => {
    expect(normalizeRelPath('  归档\\待移动.md  ')).toBe('归档/待移动.md');
    expect(normalizeRelPath('dir//a.md')).toBe('dir/a.md');
    expect(normalizeRelPath('./dir/a.md/')).toBe('dir/a.md');
    expect(normalizeRelPath('')).toBe('');
  });

  it('isSameRelPath：按平台大小写口径比较', () => {
    expect(isSameRelPath('归档\\A.md', '归档/A.md', true)).toBe(true);
    expect(isSameRelPath('A.md', 'a.md', true)).toBe(true);
    expect(isSameRelPath('A.md', 'a.md', false)).toBe(false);
    expect(isSameRelPath('dir/a.md', 'dir/b.md', true)).toBe(false);
  });
});

describe('ensureNoteExtension：新建笔记补 .md（M5 冒烟 5.6）', () => {
  it('没有扩展名时补 .md', () => {
    expect(ensureNoteExtension('未命名')).toBe('未命名.md');
    expect(ensureNoteExtension('  读书笔记  ')).toBe('读书笔记.md');
  });

  it('已有扩展名原样返回（.md 大小写不敏感；其它扩展名不擅自改）', () => {
    expect(ensureNoteExtension('已命名.md')).toBe('已命名.md');
    expect(ensureNoteExtension('已命名.MD')).toBe('已命名.MD');
    expect(ensureNoteExtension('data.txt')).toBe('data.txt');
  });

  it('空串保持空串（让校验给出「名称不能为空」，而不是变成 .md）', () => {
    expect(ensureNoteExtension('   ')).toBe('');
  });
});

describe('wikilinkOf：FR-FILE-07 的复制内容', () => {
  it('笔记去掉 .md；附件保留扩展名', () => {
    expect(wikilinkOf('dir/笔记.md')).toBe('[[dir/笔记]]');
    expect(wikilinkOf('img/a.png')).toBe('[[img/a.png]]');
  });

  it('大写扩展名同样去掉（大小写不敏感）', () => {
    expect(wikilinkOf('A.MD')).toBe('[[A]]');
  });

  it('扩展名识别只看最后一段（隐藏文件视为无扩展名）', () => {
    expect(extensionOf('.gitignore')).toBe('');
    expect(extensionOf('a.tar.gz')).toBe('gz');
    expect(extensionOf('README')).toBe('');
  });
});
