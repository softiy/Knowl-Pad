<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useVaultStore } from '@features/vault';
import { EditorView, useEditorStore } from '@features/editor';
import { FileOpsDialog, FileTree, useFileOpsStore, type MenuAction } from '@features/file-tree';
import { LinksPanel } from '@features/links';
import VaultList from '@features/vault/components/VaultList.vue';
import StatusBar from './StatusBar.vue';

/**
 * 工作区布局（技术方案 §2.2 的 `app/layout`）：左文件树 + 右编辑器 + 文件操作对话框 + 状态栏。
 *
 * 接线职责（M2 的最后一公里）：
 * - 文件树点开笔记 → 编辑器打开（FR-FILE-10 的进入编辑态）；
 * - 文件树右键动作 → 文件操作对话框（新建/重命名/删除，AC-FILE-03/04）；
 * - 重命名成功后由 fileOps store 调 `editor.followRename`（FR-FILE-28 / AC-FILE-05）；
 * - 状态栏显示 Vault 名称与索引状态（FR-VAULT-11）与 Git 忽略提示（FR-VAULT-12/FR-STORAGE-03）。
 * 未打开 Vault 时展示 Vault 列表（避免空白工作区）。
 */
const vaultStore = useVaultStore();
const ops = useFileOpsStore();
const editor = useEditorStore();

// Vault 状态来自 store（唯一来源）：切换/打开/移除都会响应式传播，
// 状态栏的 watch 因此真正生效（M1/M2 复核：此前只在挂载时取一次）。
onMounted(() => {
  void vaultStore.init();
});

/** 切换失败时的可见提示（R-07：不静默丢内容，也不假装成功）。 */
const switchError = ref<string | null>(null);

/**
 * FR-VAULT-04/05：回到欢迎页以切换知识库。
 *
 * 顺序很关键：**先刷盘，再关闭** —— 否则未保存内容会随 Vault 关闭一起消失
 * （AC-VAULT-03 的"编辑内容被写入磁盘"也正是这条路径）。
 * 若仍有标签保存失败，则**取消切换**并给出可见提示：宁可不切，也不丢内容。
 */
async function onSwitchVault(): Promise<void> {
  switchError.value = null;
  await editor.saveAll();
  if (editor.hasUnsaved()) {
    // 脏判定用 store 的派生函数（buffer 本身没有 isDirty 字段）。
    const dirty = editor.buffers.filter((b) => editor.isDirty(b.relPath)).length;
    switchError.value = `仍有 ${dirty} 个标签保存失败，已取消切换；请处理后重试。`;
    return;
  }
  await vaultStore.closeCurrent();
}

function onOpen(relPath: string): void {
  void editor.openNote(relPath);
}

/** 点击反链/孤立项：打开来源笔记（FR-LINK-13 的第一半；滚动定位与高亮见遗留项）。 */
function onOpenLink(payload: { relPath: string; line: number }): void {
  void editor.openNote(payload.relPath);
}

function parentOf(relPath: string, isDir: boolean): string {
  if (isDir) return relPath;
  const index = relPath.lastIndexOf('/');
  return index < 0 ? '' : relPath.slice(0, index);
}

function onAction(payload: { type: MenuAction; relPath: string; isDir: boolean }): void {
  const parent = parentOf(payload.relPath, payload.isDir);
  if (payload.type === 'newNote') ops.openNewNote(parent);
  else if (payload.type === 'newFolder') ops.openNewFolder(parent);
  else if (payload.type === 'rename') ops.openRename(payload.relPath);
  else if (payload.type === 'delete') ops.openDelete(payload.relPath);
}
</script>

<template>
  <div class="kp-workspace">
    <p v-if="vaultStore.error" class="kp-workspace__error">{{ vaultStore.error }}</p>
    <header v-if="vaultStore.current" class="kp-workspace__bar">
      <span class="kp-workspace__vault" data-testid="workspace-vault-name">
        {{ vaultStore.current.displayName }}
      </span>
      <button
        type="button"
        class="kp-workspace__switch"
        data-testid="workspace-switch-vault"
        @click="onSwitchVault"
      >
        切换知识库
      </button>
    </header>
    <p v-if="switchError" class="kp-workspace__error" data-testid="workspace-switch-error">
      {{ switchError }}
    </p>
    <div class="kp-workspace__body">
      <template v-if="vaultStore.current">
        <aside class="kp-workspace__side">
          <FileTree @open="onOpen" @action="onAction" />
        </aside>
        <section class="kp-workspace__main">
          <EditorView />
        </section>
        <LinksPanel :active-rel-path="editor.activeRelPath ?? null" @open="onOpenLink" />
      </template>
      <VaultList v-else />
    </div>
    <FileOpsDialog />
    <p v-if="ops.notice" class="kp-workspace__notice" data-testid="workspace-notice">
      {{ ops.notice }}
    </p>
    <StatusBar :vault="vaultStore.current" />
  </div>
</template>

<style scoped>
.kp-workspace { display: flex; flex-direction: column; height: 100vh; }
.kp-workspace__body { flex: 1; display: flex; min-height: 0; }
.kp-workspace__side { width: 280px; min-width: 200px; border-right: 1px solid var(--kp-border, #ddd); overflow: hidden; }
.kp-workspace__main { flex: 1; min-width: 0; }
.kp-workspace__error { margin: 0; padding: 6px 8px; color: #b00020; }
.kp-workspace__notice { position: fixed; bottom: 32px; left: 50%; transform: translateX(-50%); margin: 0; padding: 6px 12px; background: #e8f5e9; border-radius: 4px; font-size: 12px; }
</style>
