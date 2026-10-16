<script setup lang="ts">
import { ref } from 'vue';
import RewriteConfirmPanel from '@shared/components/RewriteConfirmPanel.vue';
import { tagRenameApply, tagRenamePreview } from '@core/ipc/tagCommands';
import { asKpError, isInvalidNameError, isPreviewExpired } from '@core/ipc/errors';
import type { RewritePreview, RewriteResult } from '@core/ipc/linkCommands';

/**
 * 标签批量重命名（FR-TAG-07/08；AC-TAG-02）。
 *
 * 与链接域一样是**薄容器**：两步确认的骨架在 `@shared/components/RewriteConfirmPanel.vue`，
 * 这里只负责三件标签域特有的事：
 * ① 用**标签名**（而不是 `[[链接]]`）作为输入与文案 —— `unit-label` 让确认句说的是"处标签"；
 * ② 层级冲突（FR-TAG-08）走 `E_INVALID_FILENAME`：**后端拒绝、磁盘零改动**，这里原样透出原因；
 * ③ 预览过期（`E_PREVIEW_EXPIRED`）要给"重新预览"的**可操作**提示：
 *    它表示 `previewId` 已被消费/失效（例如用户点了两次"确认执行"），**不是**改写失败。
 */
const props = defineProps<{ fromTag?: string | undefined }>();

const emit = defineEmits<{ renamed: [result: RewriteResult] }>();

/** 预览过期后**锁住**确认按钮：必须重新预览，否则用户会一直点一个注定失败的按钮。 */
const needsRepreview = ref(false);

/** 预览里逐处差异（面板持有原件，`#preview-items` 插槽把它交给域自己渲染）。 */
function editsOf(raw: unknown): RewritePreview['edits'] {
  return (raw as RewritePreview | null)?.edits ?? [];
}

function messageOf(err: unknown): string {
  return asKpError(err).message;
}

/** 第一步：只读预览。非法的新名（层级冲突/空/含空白）在这里就被拒绝，一个字节都不写盘。 */
async function onPreview(from: string, to: string): Promise<unknown> {
  needsRepreview.value = false;
  try {
    return await tagRenamePreview(from.trim(), to.trim());
  } catch (err) {
    // 层级冲突（FR-TAG-08）是可预期的用户输入错误：把后端原因原样带出来，别包装成"系统错误"。
    if (isInvalidNameError(err)) throw new Error(`新名称不合法：${messageOf(err)}`);
    throw err;
  }
}

async function onApply(from: string, to: string, raw: unknown): Promise<RewriteResult> {
  const preview = raw as RewritePreview;
  try {
    return await tagRenameApply(from.trim(), to.trim(), preview.previewId);
  } catch (err) {
    // 预览已过期/被消费 ⇒ 提示**重新预览**（PRD §5.2 的 410 处置），并锁住确认按钮。
    if (isPreviewExpired(err)) {
      needsRepreview.value = true;
      throw new Error('预览已过期（previewId 一次性消费），请重新点击"预览"后再确认。');
    }
    throw err;
  }
}
</script>

<template>
  <RewriteConfirmPanel
    :on-preview="onPreview"
    :on-apply="onApply"
    :can-apply="!needsRepreview"
    from-placeholder="旧标签名（如 old/name）"
    to-placeholder="new/name"
    to-label="新名称"
    unit-label="标签"
    @applied="emit('renamed', $event as RewriteResult)"
  >
    <template #preview-items="{ preview }">
      <li v-for="e in editsOf(preview)" :key="e.relPath">
        {{ e.relPath }}（{{ e.hits }} 处）
        <ul v-if="e.occurrences.length" data-testid="tag-rename-preview-diff">
          <li v-for="(o, i) in e.occurrences" :key="e.relPath + '-' + i">
            第 {{ o.line }} 行：<code>{{ o.before }}</code> → <code>{{ o.after }}</code>
          </li>
        </ul>
      </li>
    </template>

    <template #inputs="{ previewing }">
      <p v-if="props.fromTag" class="kp-tag-rename__hint" data-testid="tag-rename-from-hint">
        已选中：<code>{{ props.fromTag }}</code>（层级子标签的前缀会一并更新，FR-TAG-07）
      </p>
      <p v-if="needsRepreview" class="kp-tag-rename__warn" data-testid="tag-rename-repreview">
        预览已过期，请重新预览后再确认。
      </p>
      <p v-if="previewing" class="kp-tag-rename__hint" data-testid="tag-rename-previewing">
        正在扫描全部笔记…
      </p>
    </template>
  </RewriteConfirmPanel>
</template>

<style scoped>
.kp-tag-rename__to { display: block; }
.kp-tag-rename__hint { color: #666; font-size: 12px; margin: 2px 0; }
.kp-tag-rename__warn { color: #b26a00; background: #fff4e5; font-size: 12px; margin: 2px 0; padding: 4px 6px; }
</style>
