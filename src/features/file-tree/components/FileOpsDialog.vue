<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { useFileOpsStore } from '../stores/fileOps';

/** 文件操作对话框（AC-FILE-03 非法名原因 / AC-FILE-04 冲突三选项 / 删除确认）。 */
const ops = useFileOpsStore();
const titles: Record<string, string> = {
  newNote: '新建笔记',
  newFolder: '新建文件夹',
  rename: '重命名',
  move: '移动到…',
  delete: '移入回收站',
};
const title = computed(() => (ops.dialog ? titles[ops.dialog.mode] : ''));
/**
 * 有引用时按钮改成「仍然删除」：**多一次明确的确认动作**（FR-FILE-32「需用户确认」），
 * 没有引用时保持「确定」（不制造噪声）。
 */
const submitLabel = computed(() =>
  ops.dialog?.mode === 'delete' && (ops.dialog.referenceCount ?? 0) > 0 ? '仍然删除' : '确定',
);

/** 名称/路径输入框：对话框打开时必须**自动聚焦**（M5 冒烟 5.6：此前需要用户手动点）。 */
const nameInput = ref<HTMLInputElement | null>(null);

/**
 * 打开对话框（或切换模式）后：聚焦输入框，并把**主文件名**选中。
 *
 * 为什么只选中主文件名而不是整个值：新建笔记的默认值是「未命名.md」——整段选中时用户一输入
 * 就把扩展名也删掉了（又回到无扩展名问题）；只选「未命名」则扩展名始终可见、也无需用户自己补。
 * 移动输入的是完整**路径**，整体选中更顺手。
 */
watch(
  // 监听**对话框对象本身**而不是 mode：连续两次"新建笔记"时 mode 没变，但用户仍期望聚焦新对话框
  () => ops.dialog,
  async (dialog) => {
    if (!dialog || dialog.mode === 'delete') return;
    await nextTick();
    const input = nameInput.value;
    if (!input) return;
    input.focus();
    if (dialog.mode === 'move') {
      input.select();
      return;
    }
    const stem = input.value.toLowerCase().endsWith('.md') ? input.value.length - 3 : input.value.length;
    input.setSelectionRange(0, stem);
  },
);
</script>

<template>
  <div v-if="ops.dialog" class="kp-fileops" data-testid="fileops-dialog">
    <div class="kp-fileops__panel">
      <h3 class="kp-fileops__title">{{ title }}</h3>

      <p v-if="ops.dialog.mode === 'delete'" data-testid="fileops-delete-hint">
        确定把「{{ ops.dialog.target }}」移入回收站吗？该操作不会删除磁盘内容，可在回收站恢复。
      </p>
      <!-- FR-FILE-32 / AC-FILE-11 / DEBT-29：删除前用既有反链查询提示"被 N 处引用" -->
      <p
        v-if="ops.dialog.mode === 'delete' && (ops.dialog.referenceCount ?? 0) > 0"
        class="kp-fileops__warn"
        data-testid="delete-references"
      >
        该笔记被 {{ ops.dialog.referenceCount }} 处引用，删除后这些链接将变为悬空链接。
      </p>
      <!-- R-15：统计失败不静默、也不用 0 冒充（0 表示确实没有引用） -->
      <p
        v-if="ops.dialog.mode === 'delete' && ops.dialog.referenceError"
        class="kp-fileops__warn"
        data-testid="delete-references-error"
      >
        无法统计引用（{{ ops.dialog.referenceError }}），删除前请自行确认。
      </p>
      <!-- FR-FILE-07 / DEBT-19：file_move 的 UI 入口（移动到…） -->
      <p v-if="ops.dialog.mode === 'move'" class="kp-fileops__hint" data-testid="fileops-move-hint">
        目标路径要含文件名（例如 笔记/归档.md）。跨目录即移动；目标与当前位置相同则不会执行。
      </p>
      <label v-if="ops.dialog.mode !== 'delete'" class="kp-fileops__field">
        {{ ops.dialog.mode === 'move' ? '目标路径' : '名称' }}
        <input
          ref="nameInput"
          v-model="ops.dialog.value"
          :data-testid="ops.dialog.mode === 'move' ? 'fileops-move-target' : 'fileops-name'"
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
      <!-- 红字：校验失败 / 命令失败 / 「与当前相同」（M5 冒烟 5.4 的拦截提示也走这里） -->
      <p v-if="ops.dialog.error" class="kp-fileops__error" data-testid="fileops-error">
        {{ ops.dialog.error }}
      </p>

      <div v-if="ops.pendingDelete" class="kp-fileops__conflict" data-testid="delete-confirm">
        <p>
          「{{ ops.pendingDelete.target }}」下有 {{ ops.pendingDelete.open.length }} 个打开的标签（其中
          {{ ops.pendingDelete.dirty.length }} 个未保存）。删除会一并关闭它们：
        </p>
        <button type="button" data-testid="delete-save" @click="ops.resolveDelete('save')">保存后删除</button>
        <button type="button" data-testid="delete-discard" @click="ops.resolveDelete('discard')">直接删除</button>
        <button type="button" data-testid="delete-cancel" @click="ops.resolveDelete('cancel')">取消</button>
      </div>

      <div v-if="ops.dialog.conflict" class="kp-fileops__conflict" data-testid="fileops-conflict">
        <p>目标已存在。覆盖前会<strong>自动备份原文件</strong>，请选择处理方式：</p>
        <button type="button" data-testid="conflict-overwrite" @click="ops.resolveConflict('overwrite')">覆盖</button>
        <button type="button" data-testid="conflict-rename-new" @click="ops.resolveConflict('renameNew')">重命名新建</button>
        <button type="button" data-testid="conflict-cancel" @click="ops.resolveConflict('cancel')">取消</button>
      </div>

      <div class="kp-fileops__actions">
        <button type="button" data-testid="fileops-submit" :disabled="ops.dialog.busy" @click="ops.submit()">
          {{ submitLabel }}
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
.kp-fileops__field input { flex: 1; min-width: 0; }
.kp-fileops__hint { margin: 0 0 6px; color: #666; }
.kp-fileops__warn { margin: 6px 0 0; color: #b00020; font-weight: 600; }
.kp-fileops__error { margin: 6px 0 0; color: #b00020; }
.kp-fileops__conflict { margin-top: 8px; padding: 8px; background: #fff8e1; border-radius: 4px; }
.kp-fileops__conflict button { margin-right: 6px; }
.kp-fileops__actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
</style>
