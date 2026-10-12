<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { fileTree, preferenceGet, preferenceSet, type VaultInfo } from '@core/ipc/commands';
import { asKpError } from '@core/ipc/errors';
import IndexBadge from './IndexBadge.vue';

/**
 * 状态栏（FR-VAULT-11）与 Git 忽略提示（FR-VAULT-12 / FR-STORAGE-03）。
 *
 * **红线**（R-07 / FR-STORAGE-03）：只**提示**，绝不自动修改用户的 `.gitignore`；
 * Git 仓库检测也是只读的（列出隐藏项后看有无 `.git` 目录）。
 *
 * 索引状态与重建/取消入口在子组件 `IndexBadge`（拆分的理由见那里）。
 */
const props = defineProps<{ vault: VaultInfo | null }>();
const isGitRepo = ref(false);
const hintDismissed = ref(true);
const copied = ref(false);
const gitCheckFailed = ref(false);
const gitError = ref<string | null>(null);

/** 忽略规则（只复制给用户，不落盘）。 */
const GITIGNORE_RULE = '.knowlpad/';
const PREF_GIT_HINT_DISMISSED = 'ui.gitHintDismissed';

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
    copyError.value = null;
    gitCheckFailed.value = false;
    gitError.value = null;
    void refresh();
  },
);

/** 复制失败时的可操作提示（审计 F4：此前失败也显示"已复制"）。 */
const copyError = ref<string | null>(null);

async function copyRule(): Promise<void> {
  // 审计 F4：此前先置 copied=true 再 await —— 剪贴板被浏览器拒绝（非安全上下文/无权限）时，
  // 界面照旧显示"已复制"，用户以为规则已在剪贴板里；而且那条 rejection 也无人接。
  // 现在只有**确实写成功**才算复制，失败给出可操作提示（FR-GIT-02 的提示本来就要求用户手工改）。
  try {
    if (!navigator.clipboard) throw new Error('当前环境未提供剪贴板 API');
    await navigator.clipboard.writeText(GITIGNORE_RULE);
    copied.value = true;
    copyError.value = null;
  } catch {
    copied.value = false;
    copyError.value = '复制失败，请手动把 .knowlpad/ 加入 .gitignore';
  }
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
    <IndexBadge :vault="props.vault" />

    <span v-if="gitCheckFailed" class="kp-status__hint" data-testid="git-check-failed" :title="gitError ?? ''">
      Git 检测失败
    </span>

    <div v-if="isGitRepo && !hintDismissed" class="kp-status__hint" data-testid="git-hint">
      <span>检测到 Git 仓库：建议把 .knowlpad/ 加入忽略列表（本软件不会自动修改你的 .gitignore）</span>
      <button type="button" data-testid="git-hint-copy" @click="copyRule">
        {{ copied ? '已复制' : '复制忽略规则' }}
      </button>
      <span v-if="copyError" class="kp-status__hint" data-testid="git-hint-copy-error">{{ copyError }}</span>
      <button type="button" data-testid="git-hint-dismiss" @click="dismissHint">知道了</button>
    </div>
  </footer>
</template>

<style scoped>
.kp-status { display: flex; align-items: center; gap: 12px; padding: 2px 8px; border-top: 1px solid var(--kp-border, #ddd); }
.kp-status__path { color: #888; }
.kp-status__hint { display: flex; align-items: center; gap: 8px; color: #b26a00; }
</style>
