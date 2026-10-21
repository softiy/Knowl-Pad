/** 文件树纯逻辑（FR-FILE-02 排序 / FR-FILE-07 Wikilink；DEBT-19）。 */
import { describe, expect, it } from 'vitest';
import type { FileNode } from '@core/ipc/commands';
import { extensionOf, sortNodes, wikilinkOf } from '@features/file-tree/logic';

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
