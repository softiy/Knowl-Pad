import { describe, expect, it } from 'vitest';
import {
  bumpVersion,
  groupCommits,
  inferBump,
  parseHeader,
  parseLog,
  prependSection,
  renderChangelog,
} from '../../scripts/changelog.mjs';

const raw = [
  'a1b2c3d\x1ffeat(editor): 支持 wikilink 补全\x1f\x1e',
  'b2c3d4e\x1ffix: 修正重命名时的悬空链接\x1f\x1e',
  'c3d4e5f\x1fchore(deps): 升级 vite\x1f\x1e',
  'd4e5f6a\x1fdocs: 修正错别字\x1f\x1e',
  'e5f6a7b\x1frefactor(link)!: 改写器改为两阶段\x1fBREAKING CHANGE: 预览与执行分离\x1e',
  'f6a7b8c\x1fchore(release): 1.0.0 [skip ci]\x1f\x1e',
].join('');

describe('parseHeader', () => {
  it('解析类型/范围/主题', () => {
    expect(parseHeader('feat(editor): 支持补全')).toEqual({ type: 'feat', scope: 'editor', bang: false, subject: '支持补全' });
  });
  it('识别破坏性标记', () => {
    expect(parseHeader('refactor(link)!: 两阶段')?.bang).toBe(true);
  });
  it('非规范标题返回 null', () => {
    expect(parseHeader('随手写的提交')).toBeNull();
  });
});

describe('parseLog', () => {
  const commits = parseLog(raw);
  it('过滤版本提交与 [skip ci]', () => expect(commits).toHaveLength(5));
  it('保留短 hash', () => expect(commits[0].hash).toBe('a1b2c3d'));
  it('正文中的 BREAKING CHANGE 也算破坏性', () => expect(commits.find((c) => c.type === 'refactor')?.breaking).toBe(true));
});

describe('inferBump / bumpVersion', () => {
  it('有破坏性 → major', () => expect(inferBump(parseLog(raw))).toBe('major'));
  it('仅 feat → minor', () => expect(inferBump(parseLog('a\x1ffeat: x\x1f\x1e'))).toBe('minor'));
  it('仅 fix → patch', () => expect(inferBump(parseLog('a\x1ffix: x\x1f\x1e'))).toBe('patch'));
  it('无提交 → null', () => expect(inferBump([])).toBeNull());
  it('递增', () => {
    expect(bumpVersion('1.2.3', 'major')).toBe('2.0.0');
    expect(bumpVersion('1.2.3', 'minor')).toBe('1.3.0');
    expect(bumpVersion('1.2.3', 'patch')).toBe('1.2.4');
  });
  it('非法版本抛错', () => expect(() => bumpVersion('x', 'patch')).toThrow());
});

describe('groupCommits / renderChangelog', () => {
  const { groups, breaking } = groupCommits(parseLog(raw));
  it('只保留可见类型', () => {
    expect([...groups.keys()].sort()).toEqual(['✨ 新功能', '🐛 问题修复']);
    expect(groups.has('🔧 其他')).toBe(false);
  });
  it('破坏性变更单独汇总', () => expect(breaking).toHaveLength(1));
  it('渲染含标题与破坏性小节', () => {
    const text = renderChangelog({ version: '2.0.0', date: '2026-09-30', groups, breaking, from: 'v1.0.0', to: 'HEAD', repoUrl: 'https://example.com/r' });
    expect(text).toContain('## [2.0.0] - 2026-09-30');
    expect(text).toContain('### ⚠️ 破坏性变更');
    expect(text).toContain('**editor**: 支持 wikilink 补全');
    expect(text).toContain('compare/v1.0.0...HEAD');
  });
  it('无面向用户变更时给出说明', () => {
    const empty = groupCommits(parseLog('a\x1fchore: x\x1f\x1e'));
    expect(renderChangelog({ version: '1.0.1', date: '2026-09-30', ...empty })).toContain('本版本无面向用户的变更');
  });
});

describe('prependSection', () => {
  it('插入到首个标题之后', () => {
    const out = prependSection('# 更新日志\n\n旧内容\n', '## [1.0.0]\n');
    expect(out.startsWith('# 更新日志\n')).toBe(true);
    expect(out.indexOf('## [1.0.0]')).toBeLessThan(out.indexOf('旧内容'));
  });
  it('无标题时追加', () => expect(prependSection('', 'X')).toContain('X'));
});
