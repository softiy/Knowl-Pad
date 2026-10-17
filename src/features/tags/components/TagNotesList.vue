<script setup lang="ts">
import { computed } from 'vue';
import { formatNoteTime } from '../logic';
import type { TaggedNote, TagNode, TagSource } from '@core/ipc/tagCommands';

/**
 * 选中标签下的笔记列表（FR-TAG-03；AC-TAG-03 末句"点击该标签列出全部 3 篇笔记"）。
 *
 * 两个容易写错的点：
 * ① **笔记条数不等于 refCount**：`refCount` 是 file_tag **行数**（同一篇里写两次算两行），
 *    这里显示的是**去重后的篇数**（`tag_notes` 的列表长度）—— PRD §5.3.6.1 的"计数口径"③；
 * ② 「未分类」（`norm === ''`）是**合法选中**而不是"没选"：判据用 `activeNorm !== null`，
 *    用真值判定会让未分类条目点不出来。
 */
const props = defineProps<{
  notes: TaggedNote[];
  activeNorm: string | null;
  activeNode: TagNode | null;
  loading: boolean;
}>();

const emit = defineEmits<{ open: [relPath: string] }>();

const selected = computed(() => props.activeNorm !== null);
/** 标题：优先用索引库里的 display（首次出现的原文形式，FR-TAG-09），查不到才退化用规范键。 */
const title = computed(() => props.activeNode?.display ?? props.activeNorm ?? '');

/**
 * 来源标识（FR-TAG-10）：把后端的取值翻成用户看得懂的词。
 *
 * 用**不同的字面**而不是图标/颜色：色觉障碍用户也要能区分（这条信息是"标签从哪来"，不是装饰）。
 */
function sourceLabel(src: TagSource): string {
  return src === 'frontmatter' ? 'frontmatter' : '正文';
}
</script>

<template>
  <section class="kp-tags__notes" data-testid="tag-notes">
    <p v-if="!selected" class="kp-tags__hint" data-testid="tag-notes-hint">点击一个标签，查看它下面的笔记</p>
    <template v-else>
      <p class="kp-tags__notes-title" data-testid="tag-notes-title">
        {{ title }} —— 共 {{ props.notes.length }} 篇
      </p>
      <p v-if="props.loading" class="kp-tags__hint" data-testid="tag-notes-loading">加载中…</p>
      <p v-else-if="props.notes.length === 0" class="kp-tags__hint" data-testid="tag-notes-empty">
        这个标签下没有笔记（若它是孤儿标签，可触发索引重建清理：勘误 D-41/D-42）
      </p>
      <ul v-else class="kp-tags__notes-list">
        <li v-for="note in props.notes" :key="note.relPath">
          <button type="button" data-testid="tag-note-item" @click="emit('open', note.relPath)">
            {{ note.name }}
          </button>
          <span class="kp-tags__note-path">{{ note.relPath }}</span>
          <span
            v-for="src in note.sources"
            :key="src"
            class="kp-tags__note-source"
            data-testid="tag-note-source"
            :data-source="src"
          >{{ sourceLabel(src) }}</span>
          <span class="kp-tags__note-time">{{ formatNoteTime(note.mtimeMs) }}</span>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.kp-tags__notes { border-top: 1px solid var(--kp-border, #eee); margin-top: 6px; padding-top: 6px; }
.kp-tags__notes-title { margin: 4px 0; font-size: 12px; font-weight: 600; }
.kp-tags__notes-list { list-style: none; margin: 0; padding: 0; }
.kp-tags__notes-list li { display: flex; gap: 6px; align-items: baseline; font-size: 12px; }
.kp-tags__note-path { color: #888; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.kp-tags__note-source {
  border: 1px solid var(--kp-border, #e5e5e5);
  border-radius: 3px;
  padding: 0 3px;
  color: #666;
  font-size: 10px;
}
.kp-tags__note-time { margin-left: auto; color: #aaa; font-size: 11px; }
.kp-tags__hint { color: #888; font-size: 12px; }
</style>
