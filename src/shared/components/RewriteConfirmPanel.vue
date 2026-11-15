<script setup lang="ts" generic="T extends RewriteOutcome = RewriteOutcome">
import { ref } from 'vue';

/**
 * 改写的**两步确认**面板（FR-FILE-21 + PRD §5.3.3 的 `preview_id` 流程）—— **域无关的纯展示件**。
 *
 * 抽出来的理由（M5 批次 2 的 W7，方案 a）：链接域与标签域的两步确认是**同一段逻辑**
 * （第一步只读预览 → 用户确认 → 一次性消费 previewId → 失败可回滚）。
 * 若各写一份，"闸门"就有两处真相：一处加了防护、另一处忘了，用户看到的确认文案也会不一致。
 *
 * 边界：本组件**不 import 任何 `core/ipc`**，也不认识 `RewritePreview`：
 * 三个动作以回调注入（`onPreview` / `onApply` / `onRollback`），域特有的输入区走 `#inputs` 插槽。
 * 这样两个域名自保留自己的 IPC 形状与参数，而"预览 → 确认"的骨架只有一份。
 */
/**
 * 本面板唯一关心的**两半**执行结果（PRD §5.3.6.1 的 `RewriteResult` 就是这两半 + `operationId`）。
 *
 * 域可以用 `generic` 参数把精确类型补给面板（链接域与标签域都给 `RewriteResult`），
 * 于是 `@applied` 的 `$event` 在调用方就是那个精确类型 —— 面板**不假装知道** `operationId` 之类的东西。
 */
export interface RewriteOutcome {
  fileCount: number;
  spanCount: number;
}

/** 面板内部只需要上面两半；`T` 的其余字段原样透传给 `applied`。 */
type OutcomeCounts = { fileCount: number; spanCount: number };

// 两个输入框**缺省就渲染**：两步确认的第一句永远是"把 A 改成 B"，
// 只有"目标由别处给"的调用方才关掉它（缺省 false 会让调用方一忘就退化成没有输入区的空面板）。
// `generic` 让**域自己**决定执行结果的精确类型（链接域与标签域返回的都是 `RewriteResult`）：
// 泛型由 `onApply` 的声明返回类型推断，于是 `@applied` 的 `$event` 在调用方就是那个精确类型，
// 不需要任何 `as unknown as`（那等于把"对端给了 operationId"这条事实藏起来）。
const props = withDefaults(defineProps<{
  /** 要求用户输入"源"（链接域是链接目标，标签域是标签名）。 */
  askFrom?: boolean;
  /** 要求用户输入"目标"。 */
  askTo?: boolean;
  /** 有值时渲染"源"输入框的占位符。 */
  fromPlaceholder?: string;
  /** 有值时渲染"目标"输入框的占位符。 */
  toPlaceholder?: string;
  /** 目标输入框前面的标签（域不同：链接域是链接目标，标签域是"新名称"）。 */
  toLabel?: string;
  /** 是否允许"确认执行"（缺省允许）。
   *
   * 由调用方控制的两个真实场景：预览已过期必须先重新预览（`E_PREVIEW_EXPIRED`）；
   * 以及标签重命名还没填新名称。**锁住按钮比让用户撞一个注定失败的调用更诚实**。 */
  canApply?: boolean;
  /** 计量单位文案（链接域"处链接"、标签域"处标签"）：让确认文案说的是用户看得到的东西。 */
  unitLabel?: string;
  /**
   * 执行期错误的**分类**（缺省一律 other）。唯一用途：把「预览已过期」与真正的改写失败
   * 在视觉上分开 —— 前者是**可操作**的（重新预览即可，橙色提示），后者才是红色失败。
   * 面板刻意不 import core/ipc，因此判定由域注入（标签域用 isPreviewExpired）。
   */
  classifyApplyError?: ((err: unknown) => 'expired' | 'other') | undefined;
  /** 第一步：**只读**预览。必须由调用方注入（本组件不碰 IPC）。 */
  onPreview: (from: string, to: string) => Promise<unknown>;
  /** 第二步：用户确认后执行。缺失则只做预览（只读模式）。返回类型决定 `T`。 */
  onApply?: ((from: string, to: string, preview: unknown) => Promise<T>) | undefined;
  /**
   * 从备份回滚。缺失则不显示回滚按钮。
   *
   * 入参只有**本次执行的结果**（含 `operationId`）：回滚凭的是执行凭据，
   * 不是预览 —— 预览在执行成功后就作废了（一次性消费 previewId）。
   */
  onRollback?: ((outcome: T) => Promise<void>) | undefined;
  /** 按钮文案（域不同：链接域"批量改写"、标签域"重命名标签"）。 */
  actionLabel?: string;
}>(), {
  askFrom: true,
  askTo: true,
  // 显式给出可选 prop 的缺省值：项目开了 exactOptionalPropertyTypes 且 lint 禁止无缺省的可选 prop，
  // 由组件自己兜底比"调用方必须传 undefined"更不容易在新增调用点时漏掉。
  fromPlaceholder: 'A',
  toPlaceholder: 'B',
  unitLabel: '内容',
  toLabel: '改成',
  canApply: true,
  actionLabel: '批量改写',
  classifyApplyError: undefined,
  onApply: undefined,
  onRollback: undefined,
});


/**
 * 缺省的错误分类：只按**线上错误码**判"预览已过期"。
 *
 * 面板刻意不 import core/ipc（域无关），但 E_PREVIEW_EXPIRED 是 PRD §5.2 定义的**跨域契约**
 * （链接改写与标签重命名共用同一个码）。有了它，两个域即使不注入 classifyApplyError，
 * 第二次「确认执行」也不会被渲染成"改写失败"那种红色告警。
 */
function defaultClassifyApplyError(err: unknown): 'expired' | 'other' {
  const code = (err as { code?: unknown } | null)?.code;
  return code === 'E_PREVIEW_EXPIRED' ? 'expired' : 'other';
}

const from = ref('');
const to = ref('');
/** 预览结果**原样**持有：本组件不认识它的形状，只负责"有预览才能确认"。 */
const preview = ref<unknown>(null);
/** 执行结果（精确类型 `T`）；回滚成功后清空，故回滚仍用它作凭据。 */
const applied = ref<T | null>(null);
const error = ref<string | null>(null);
/** 错误的呈现类别：expired ⇒ 橙色「预览已过期」；other ⇒ 红色失败。 */
const errorKind = ref<'expired' | 'other'>('other');
const busy = ref(false);

/** `unknown` 的最小结构收窄（不用 any：CODE-01）。 */
function previewOf(value: unknown): { fileCount: number; spanCount: number; skipped: unknown[] } {
  if (typeof value !== 'object' || value === null) return { fileCount: 0, spanCount: 0, skipped: [] };
  const rec = value as Record<string, unknown>;
  return {
    fileCount: typeof rec.fileCount === 'number' ? rec.fileCount : 0,
    spanCount: typeof rec.spanCount === 'number' ? rec.spanCount : 0,
    // 序列化边界要容忍字段缺失：mock 或更早版本的对端可能不带 skipped，
    // 用 `?? []` 而不是 `preview.skipped.length`（链接域曾因此在 CI 上运行期崩过）。
    skipped: Array.isArray(rec.skipped) ? rec.skipped : [],
  };
}

/** 需要用户显式确认的两个输入（第三步的回滚不需要）。 */
async function runPreview(): Promise<void> {
  busy.value = true;
  error.value = null;
  // 重新预览就是从"过期"回到可用：上一轮的橙色警示必须消失（C7 的"再点一次预览恢复"）。
  errorKind.value = 'other';
  try {
    preview.value = await props.onPreview(from.value, to.value);
  } catch (err) {
    preview.value = null;
    error.value = describe(err);
  } finally {
    busy.value = false;
  }
}

async function runApply(): Promise<void> {
  if (!props.onApply || preview.value === null) return;
  busy.value = true;
  error.value = null;
  try {
    const result = await props.onApply(from.value, to.value, preview.value);
    applied.value = result;
    // **成功后不清 preview**：M5 冒烟 C6 要求「预览成功后连点两次确认」时第二次能撞上
    // E_PREVIEW_EXPIRED（previewId 一次性消费由后端保证）。清空 preview 会让"确认执行"
    // 按钮随 v-if 一起消失，用户根本没有第二次点击的机会 —— 那条失败路径就永远无法复现、
    // 也无法验证。保留 preview 也让用户仍看得见本次改写的清单（不是假装什么都没发生）。
    // 真正的"再来一次"必须先重新预览（C7）：第二次点击会得到过期错误并锁住按钮。
    emit('applied', result);
  } catch (err) {
    errorKind.value = (props.classifyApplyError ?? defaultClassifyApplyError)(err);
    error.value = describe(err);
  } finally {
    busy.value = false;
  }
}

async function runRollback(): Promise<void> {
  // 判据是 `applied`（执行成功过）而不是 `preview`：执行成功后 preview 已被清空，
  // 用 preview 判定会让回滚按钮**永远点不动**（既有链接域用例曾据此抓到本缺陷）。
  if (!props.onRollback || applied.value === null) return;
  busy.value = true;
  error.value = null;
  errorKind.value = 'other'; // 回滚失败是普通失败，不是"预览过期"
  try {
    await props.onRollback(applied.value);
    applied.value = null;
  } catch (err) {
    error.value = describe(err);
  } finally {
    busy.value = false;
  }
}

/** 模板里的点击处理器是同步函数（避免返回 Promise 造成"无人处理的拒绝"）。 */
function onRollbackClick(): void {
  void runRollback();
}

const emit = defineEmits<{ applied: [outcome: T] }>();

/** 只读地取本面板关心的两半（`T` 的其余字段不归本组件管）。
 *
 * 参数收在 `RewriteOutcome`（`T` 的约束）而不是 `T`：泛型在**模板**里会被 ref 解包成
 * 一个不能证明等于 `T` 的类型，写 `T` 会得到 TS2345 的"可能被实例化成别的子类型"。
 */
function countsOf(value: RewriteOutcome): OutcomeCounts {
  return value;
}

/** 错误文案统一走 KpError.message（R-15：不空吞、原样透出后端说明）。 */
function describe(err: unknown): string {
  const message = (err as { message?: unknown } | null)?.message;
  return typeof message === 'string' && message ? message : String(err);
}
</script>

<template>
  <div class="kp-rewrite" data-testid="rewrite-dialog">
    <div v-if="props.askFrom || props.askTo" class="kp-rewrite__inputs">
      <label v-if="props.askFrom">
        <input v-model="from" data-testid="rewrite-from" :placeholder="props.fromPlaceholder ?? 'A'">
      </label>
      <span class="kp-rewrite__arrow">→</span>
      <label v-if="props.askTo">
        {{ props.toLabel }}
        <input v-model="to" data-testid="rewrite-to" :placeholder="props.toPlaceholder ?? 'B'">
      </label>
      <button
        type="button"
        :disabled="busy || !from || !to"
        data-testid="rewrite-preview"
        @click="runPreview"
      >
        预览
      </button>
    </div>

    <!-- 域特有的附加输入（链接域的"同时改名"、标签域的冲突提示等） -->
    <slot
      name="inputs"
      :from="from"
      :to="to"
      :preview="preview"
      :busy="busy"
      :previewing="busy"
    />

    <p v-if="preview !== null" data-testid="rewrite-preview-summary">
      将修改 {{ previewOf(preview).fileCount }} 个文件中的
      {{ previewOf(preview).spanCount }} 处{{ props.unitLabel ?? '内容' }}
    </p>
    <ul v-if="preview !== null" data-testid="rewrite-preview-items">
      <slot name="preview-items" :preview="preview" />
    </ul>
    <div v-if="previewOf(preview).skipped.length" class="kp-rewrite__warn" data-testid="rewrite-skipped">
      <p>有 {{ previewOf(preview).skipped.length }} 个文件**不会被处理**：</p>
      <ul>
        <li v-for="(s, i) in previewOf(preview).skipped" :key="i">{{ s }}</li>
      </ul>
      <p>它们里面指向旧名的引用会保持原样，执行后会变成悬空引用。</p>
    </div>
    <button
      v-if="preview !== null && props.onApply"
      type="button"
      :disabled="busy || !props.canApply"
      data-testid="rewrite-apply"
      @click="runApply"
    >
      确认执行
    </button>

    <p v-if="applied" data-testid="rewrite-applied">
      已改写 {{ countsOf(applied).fileCount }} 个文件 / {{ countsOf(applied).spanCount }} 处{{ props.unitLabel ?? '内容' }}
      <button
        v-if="props.onRollback"
        type="button"
        :disabled="busy"
        data-testid="rewrite-rollback"
        @click="onRollbackClick"
      >
        回滚
      </button>
    </p>
    <p v-if="error" class="kp-rewrite__error" :data-kind="errorKind" data-testid="rewrite-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.kp-rewrite { display: flex; flex-direction: column; gap: 6px; }
.kp-rewrite__inputs { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.kp-rewrite__arrow { color: #888; }
.kp-rewrite__error { color: #b00020; }
/* 「预览已过期」是可操作的（重新预览即可），不是磁盘失败：橙色警示 + 可读背景。
   红色只留给真正的改写失败 —— 否则用户会以为磁盘坏了（M5 冒烟 C6 的失败态）。 */
.kp-rewrite__error[data-kind='expired'] {
  color: #b26a00;
  background: #fff4e5;
  border: 1px solid #ffd591;
  padding: 4px 6px;
}
.kp-rewrite__warn { background: #fff7e6; border: 1px solid #ffd591; padding: 6px 8px; }
</style>
