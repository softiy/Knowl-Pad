<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import BacklinkPanel from './BacklinkPanel.vue';
import RewriteConfirmDialog from './RewriteConfirmDialog.vue';
import { useLinksStore } from '../stores/links';
import { linkResolveAmbiguous } from '@core/ipc/linkCommands';
import { asKpError } from '@core/ipc/errors';

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
/** 每组歧义链接选择的目标（按目标名分组）。 */
const chose = ref<Record<string, string>>({});
const actionError = ref<string | null>(null);

/**
 * 把该组的歧义链接**逐条**指定为目标（FR-LINK-22 / AC-LINK-05）。
 *
 * 逐条调用 `link_resolve_ambiguous`：后端按 link_id 精确改写**那一处**，不碰同文件里其它引用；
 * 某条失败不影响其余，最后汇总错误（R-15：不空吞）。
 */
async function resolveGroup(item: { targetRef: string; linkIds: number[] }): Promise<void> {
  const target = chose.value[item.targetRef];
  if (!target) return;
  actionError.value = null;
  const failed: string[] = [];
  for (const id of item.linkIds) {
    try {
      await linkResolveAmbiguous(id, target);
    } catch (err) {
      failed.push(asKpError(err).message);
    }
  }
  await links.refresh();
  if (failed.length) actionError.value = failed[0] ?? null;
}
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
    <p v-if="actionError" class="kp-links__error" data-testid="links-action-error">{{ actionError }}</p>

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
          <select v-model="chose[a.targetRef]" data-testid="ambiguous-choose">
            <option value="">选择目标…</option>
            <option v-for="c in a.candidates" :key="c" :value="c">{{ c }}</option>
          </select>
          <button
            type="button"
            :disabled="!chose[a.targetRef]"
            data-testid="ambiguous-resolve"
            @click="resolveGroup(a)"
          >
            指定为该目标（{{ a.linkIds.length }} 条）
          </button>
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
