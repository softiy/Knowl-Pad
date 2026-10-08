<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import BacklinkPanel from './BacklinkPanel.vue';
import RewriteConfirmDialog from './RewriteConfirmDialog.vue';
import { useLinksStore } from '../stores/links';

/**
 * 链接面板容器（FR-LINK-10~22）。
 *
 * 四个页签：反链（当前笔记）/ 悬空 / 歧义 / 孤立。数据全部来自索引库实时查询（FR-LINK-17），
 * store 订阅索引与文件事件并防抖刷新（1 秒内可见）。
 */
const props = defineProps<{ activeRelPath?: string | null }>();
const emit = defineEmits<{ (e: 'open', payload: { relPath: string; line: number }): void }>();

const links = useLinksStore();
const tab = ref<'backlinks' | 'dangling' | 'ambiguous' | 'orphans'>('backlinks');
/** 歧义行的候选选择（linkId → 选中的候选）。 */

onMounted(async () => {
  await links.subscribe();
  await links.setActive(props.activeRelPath ?? null);
  await links.refresh();
});
onUnmounted(() => links.dispose());

</script>

<template>
  <aside class="kp-links" data-testid="links-panel">
    <nav class="kp-links__tabs">
      <button type="button" data-testid="links-tab-backlinks" @click="tab = 'backlinks'">
        反链 {{ links.totals.backlinks }}
      </button>
      <button type="button" data-testid="links-tab-dangling" @click="tab = 'dangling'">
        悬空 {{ links.totals.dangling }}
      </button>
      <button type="button" data-testid="links-tab-ambiguous" @click="tab = 'ambiguous'">
        歧义 {{ links.totals.ambiguous }}
      </button>
      <button type="button" data-testid="links-tab-orphans" @click="tab = 'orphans'">
        孤立 {{ links.totals.orphans }}
      </button>
    </nav>

    <p v-if="links.error" class="kp-links__error" data-testid="links-error">{{ links.error }}</p>

    <section v-if="tab === 'backlinks'" data-testid="links-backlinks">
      <p v-if="!links.activeRelPath" class="kp-links__hint" data-testid="links-no-note">
        打开一篇笔记即可查看它的反链
      </p>
      <BacklinkPanel v-else :groups="links.backlinks" @open="emit('open', $event)" />
    </section>

    <section v-else-if="tab === 'dangling'" data-testid="links-dangling">
      <p v-if="links.dangling.length === 0" class="kp-links__hint">没有悬空链接</p>
      <ul>
        <li v-for="d in links.dangling" :key="d.targetRef" data-testid="dangling-item">
          <strong>{{ d.targetRef }}</strong> —— 被 {{ d.refCount }} 处引用（{{ d.sourceCount }} 篇）
        </li>
      </ul>
    </section>

    <section v-else-if="tab === 'ambiguous'" data-testid="links-ambiguous">
      <p v-if="links.ambiguous.length === 0" class="kp-links__hint">没有歧义链接</p>
      <ul>
        <li v-for="a in links.ambiguous" :key="a.targetRef + (a.candidates[0] ?? '')" data-testid="ambiguous-item">
          <strong>{{ a.targetRef }}</strong> —— {{ a.refCount }} 处引用，{{ a.candidates.length }} 个候选
          <span class="kp-links__candidates">候选：{{ a.candidates.join(' / ') }}</span>
          <!-- 顺延：逐条"指定目标"需要 link_id，而本列表按目标名分组、不带 link_id（见 PR 说明） -->
          <span class="kp-links__hint" data-testid="ambiguous-deferred">（逐条指定目标下一批开放）</span>
        </li>
      </ul>
    </section>

    <section v-else data-testid="links-orphans">
      <p v-if="links.orphans.length === 0" class="kp-links__hint">没有被孤立的笔记</p>
      <ul>
        <li v-for="o in links.orphans" :key="o" data-testid="orphan-item">
          <button type="button" @click="emit('open', { relPath: o, line: 1 })">{{ o }}</button>
        </li>
      </ul>
    </section>

    <details class="kp-links__rewrite">
      <summary>批量改写链接…</summary>
      <RewriteConfirmDialog :rename-from="links.activeRelPath ?? undefined" />
    </details>
  </aside>
</template>

<style scoped>
.kp-links { border-left: 1px solid var(--kp-border, #ddd); display: flex; flex-direction: column; gap: 8px; overflow: auto; padding: 8px; width: 320px; }
.kp-links__tabs { display: flex; flex-wrap: wrap; gap: 4px; }
.kp-links__error { color: #b00020; }
.kp-links__hint { color: #888; }
.kp-links__rewrite { margin-top: auto; }
</style>
