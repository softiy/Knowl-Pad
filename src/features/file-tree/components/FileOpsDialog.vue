<script setup lang="ts">
import { computed } from 'vue';
import { useFileOpsStore } from '../stores/fileOps';

/** 文件操作对话框（AC-FILE-03 非法名原因 / AC-FILE-04 冲突三选项 / 删除确认）。 */
const ops = useFileOpsStore();
const titles: Record<string, string> = {
  newNote: '新建笔记',
  newFolder: '新建文件夹',
  rename: '重命名',
  delete: '移入回收站',
};
const title = computed(() => (ops.dialog ? titles[ops.dialog.mode] : ''));
</script>

<template>
  <div v-if="ops.dialog" class="kp-fileops" data-testid="fileops-dialog">
    <div class="kp-fileops__panel">
      <h3 class="kp-fileops__title">{{ title }}</h3>

      <p v-if="ops.dialog.mode === 'delete'" data-testid="fileops-delete-hint">
        确定把「{{ ops.dialog.target }}」移入回收站吗？该操作不会删除磁盘内容，可在回收站恢复。
      </p>
      <label v-else class="kp-fileops__field">
        名称
        <input
          v-model="ops.dialog.value"
          data-testid="fileops-name"
          @input="ops.dialog.validation = null"
          @keyup.enter="ops.submit()"
        >
      </label>

      <p
        v-if="ops.dialog.validation && !ops.dialog.validation.valid"
        class="kp-fileops__error"
        data-testid="fileops-validation"
      >
        {{ ops.dialog.validation.reason }}
      </p>
      <p v-if="ops.dialog.error" class="kp-fileops__error" data-testid="fileops-error">
        {{ ops.dialog.error }}
      </p>

      <div v-if="ops.dialog.conflict" class="kp-fileops__conflict" data-testid="fileops-conflict">
        <p>目标已存在。覆盖前会**自动备份原文件**，请选择处理方式：</p>
        <button type="button" data-testid="conflict-overwrite" @click="ops.resolveConflict('overwrite')">覆盖</button>
        <button type="button" data-testid="conflict-rename-new" @click="ops.resolveConflict('renameNew')">重命名新建</button>
        <button type="button" data-testid="conflict-cancel" @click="ops.resolveConflict('cancel')">取消</button>
      </div>

      <div class="kp-fileops__actions">
        <button type="button" data-testid="fileops-submit" :disabled="ops.dialog.busy" @click="ops.submit()">
          确定
        </button>
        <button type="button" data-testid="fileops-cancel" @click="ops.close()">取消</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.kp-fileops { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; background: rgb(0 0 0 / 25%); z-index: 200; }
.kp-fileops__panel { min-width: 320px; max-width: 460px; padding: 16px; background: #fff; border-radius: 6px; box-shadow: 0 8px 24px rgb(0 0 0 / 20%); font-size: 13px; }
.kp-fileops__title { margin: 0 0 8px; font-size: 15px; }
.kp-fileops__field { display: flex; gap: 8px; align-items: center; }
.kp-fileops__error { margin: 6px 0 0; color: #b00020; }
.kp-fileops__conflict { margin-top: 8px; padding: 8px; background: #fff8e1; border-radius: 4px; }
.kp-fileops__conflict button { margin-right: 6px; }
.kp-fileops__actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
</style>
