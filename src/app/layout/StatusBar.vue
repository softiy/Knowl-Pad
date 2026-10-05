<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import {
  fileTree,
  indexStatus,
  preferenceGet,
  preferenceSet,
  type IndexStatus,
  type VaultInfo,
} from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';

/**
 * 状态栏（FR-VAULT-11）与 Git 忽略提示（FR-VAULT-12 / FR-STORAGE-03）。
 *
 * **红线**（R-07 / FR-STORAGE-03）：只**提示**，绝不自动修改用户的 `.gitignore`；
 * Git 仓库检测也是只读的（列出隐藏项后看有无 `.git` 目录）。
 */
const props = defineProps<{ vault: VaultInfo | null }>();
const index = ref<IndexStatus | null>(null);
const indexFailed = ref(false);
const isGitRepo = ref(false);
const hintDismissed = ref(true);
const copied = ref(false);

/** 忽略规则（只复制给用户，不落盘）。 */
const GITIGNORE_RULE = '.knowlpad/';
const PREF_GIT_HINT_DISMISSED = 'ui.gitHintDismissed';

/** FR-VAULT-11 的三种状态：就绪 / 索引中 / 索引失败。 */
const indexLabel = computed(() => {
  if (indexFailed.value) return '索引失败';
  if (!index.value) return '索引中';
  return index.value.ready ? '就绪' : '索引中';
});

onMounted(async () => {
  try {
    index.value = await indexStatus();
  } catch {
    // 命令本身失败（如数据库不可用）即视为索引失败——这是当前契约能给出的失败信号
    indexFailed.value = true;
  }
  if (!props.vault) return;
  try {
    const entries = await fileTree(undefined, true);
    isGitRepo.value = entries.some((entry) => entry.isDir && entry.name === '.git');
    if (isGitRepo.value) {
      const stored = await preferenceGet([PREF_GIT_HINT_DISMISSED]);
      hintDismissed.value = stored.values[PREF_GIT_HINT_DISMISSED] === true;
    }
  } catch (err) {
    // 检测失败不该打扰用户；只在控制台不可见处静默降级为「不提示」
    hintDismissed.value = true;
    void asKpError(err);
  }
});

async function copyRule(): Promise<void> {
  copied.value = true;
  await navigator.clipboard?.writeText(GITIGNORE_RULE);
}

async function dismissHint(): Promise<void> {
  hintDismissed.value = true;
  await preferenceSet({ [PREF_GIT_HINT_DISMISSED]: true });
}
</script>

<template>
  <footer class="kp-status" data-testid="status-bar">
    <span data-testid="status-vault">{{ props.vault ? props.vault.displayName : '未打开知识库' }}</span>
    <span v-if="props.vault" class="kp-status__path" data-testid="status-path">{{ props.vault.root }}</span>
    <span class="kp-status__index" data-testid="status-index">索引：{{ indexLabel }}</span>

    <div v-if="isGitRepo && !hintDismissed" class="kp-status__hint" data-testid="git-hint">
      <span>检测到 Git 仓库：建议把 .knowlpad/ 加入忽略列表（本软件不会自动修改你的 .gitignore）</span>
      <button type="button" data-testid="git-hint-copy" @click="copyRule">
        {{ copied ? '已复制' : '复制忽略规则' }}
      </button>
      <button type="button" data-testid="git-hint-dismiss" @click="dismissHint">知道了</button>
    </div>
  </footer>
</template>

<style scoped>
.kp-status { display: flex; align-items: center; gap: 12px; padding: 2px 8px; border-top: 1px solid var(--kp-border, #ddd); font-size: 12px; color: #444; }
.kp-status__path { color: #888; }
.kp-status__index { margin-left: auto; }
.kp-status__hint { display: flex; align-items: center; gap: 8px; color: #b26a00; }
</style>
