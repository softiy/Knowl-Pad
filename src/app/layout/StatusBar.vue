<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
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
/** 索引/ Git 检测失败的可见状态（R-15：不空吞错误，也不伪装成"没有仓库"） */
const indexError = ref<string | null>(null);
const gitCheckFailed = ref(false);
const gitError = ref<string | null>(null);

/** 忽略规则（只复制给用户，不落盘）。 */
const GITIGNORE_RULE = '.knowlpad/';
const PREF_GIT_HINT_DISMISSED = 'ui.gitHintDismissed';

/** FR-VAULT-11 的三种状态：就绪 / 索引中 / 索引失败。 */
const indexLabel = computed(() => {
  if (indexFailed.value) return '索引失败';
  if (!index.value) return '索引中';
  return index.value.ready ? '就绪' : '索引中';
});

/** 拉取索引状态；失败即视为「索引失败」（当前契约能给出的失败信号，FR-VAULT-11）。 */
async function refreshIndex(): Promise<void> {
  indexFailed.value = false;
  try {
    index.value = await indexStatus();
  } catch (err) {
    indexFailed.value = true;
    indexError.value = asKpError(err).message;
  }
}

/** 只读检测 Git 仓库 + 读取「不再提示」偏好（FR-VAULT-12 / FR-STORAGE-03）。 */
async function refreshGitHint(): Promise<void> {
  isGitRepo.value = false;
  if (!props.vault) return;
  const entries = await fileTree(undefined, true);
  isGitRepo.value = entries.some((entry) => entry.isDir && entry.name === '.git');
  if (!isGitRepo.value) return;
  const stored = await preferenceGet([PREF_GIT_HINT_DISMISSED]);
  hintDismissed.value = stored.values[PREF_GIT_HINT_DISMISSED] === true;
}

/** M9：Vault 切换（或关闭）后必须重新计算——否则状态栏会停留在上一个 Vault 的信息上。 */
async function refresh(): Promise<void> {
  await refreshIndex();
  if (!props.vault) {
    isGitRepo.value = false;
    return;
  }
  try {
    await refreshGitHint();
  } catch (err) {
    // R-15：不空吞——记录可见状态，并且不把检测失败伪装成「没有 Git 仓库」
    gitCheckFailed.value = true;
    gitError.value = asKpError(err).message;
  }
}

onMounted(() => {
  void refresh();
});

watch(
  () => props.vault?.root,
  () => {
    hintDismissed.value = false;
    copied.value = false;
    gitCheckFailed.value = false;
    gitError.value = null;
    void refresh();
  },
);

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

    <span v-if="gitCheckFailed" class="kp-status__hint" data-testid="git-check-failed" :title="gitError ?? ''">
      Git 检测失败
    </span>

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
