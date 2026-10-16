<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { renderMarkdown } from '@core/markdown/renderer';
import { applyLinkStatus, loadLinkStatus } from '@core/markdown/linkStatus';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';

/**
 * 阅读态渲染（技术方案 §7.2 ④⑤；FR-EDITOR-02/25/27、FR-TAG-05）。
 *
 * - 渲染 → 净化 → 图片策略全在 core/markdown/renderer 内完成；扩展语法（`[[x]]`/`#标签`）
 *   也在那里变成带 `data-kp-*` 的中转节点；
 * - 本组件是全项目**唯一**允许把渲染结果交给 v-html 的地方（经由 KpSafeHtml）；
 * - 事件用**容器级委托**（不逐个绑监听器）：读 `data-kp-*` 后向外抛事件，
 *   由上层决定路由/命令（远程图片只允许交给系统浏览器打开，SEC-08）；
 * - **链接状态（悬空/歧义）在渲染后打标**：一次性查该笔记的出链（索引阶段的裁决结果），
 *   把状态映射成类名打到 DOM 上（见 `core/markdown/linkStatus.ts` 的"为什么不放在渲染器里"）。
 *
 * **点击语义**（FR-EDITOR-25 与 FR-EDITOR-27 的分工，本组件的判断写在一处）：
 * 带修饰键（Ctrl/Cmd）⇒ 跳转；不带修饰键且**已知悬空** ⇒ 交给上层走创建流程；其余不动作。
 */
const props = defineProps<{
  source: string;
  /** 当前笔记的 relPath：用来查它的出链状态（缺省则不查，链接保持中性样式）。 */
  relPath?: string | undefined;
  resolveAsset?: (src: string) => string | null;
  cacheScope?: string;
}>();

const emit = defineEmits<{
  /** 点击远程图片占位符（上层应交给系统浏览器打开，应用内不加载） */
  remoteImage: [src: string];
  /** 激活内部链接（`[[x]]`/`![[x]]`）：目标、形态、"是否已判定悬空"，以及**原始事件**（要读修饰键）。
   *
   * 事件必须带出去：`Ctrl/Cmd+点击` 与普通点击在这条链上语义不同（FR-EDITOR-25 vs FR-EDITOR-27），
   * 而"有没有按修饰键"只有事件知道。键盘激活（Enter/空格）没有鼠标事件 ⇒ 传 `undefined`。 */
  link: [payload: { target: string; kind: string; dangling: boolean }, event?: MouseEvent];
  /** 激活标签（`#a/b`）：显示面板用它去选中该标签（FR-TAG-05） */
  tag: [tag: string];
}>();

const html = computed(() =>
  renderMarkdown(props.source, {
    resolveAsset: props.resolveAsset,
    cacheScope: props.cacheScope,
  }),
);
const rootRef = ref<HTMLElement | null>(null);
/** 组件存活标记：异步状态回来时组件可能已卸载（不设它会在卸载后操作 DOM）。 */
let alive = true;
/** "只认最后一次请求"：连切笔记时慢的旧响应不能把状态打到新笔记上（同搜索域/标签 store 的口径）。 */
let statusSeq = 0;

onBeforeUnmount(() => {
  alive = false;
});

/** 查该笔记的出链状态并打到 DOM 上（`nextTick` 保证 KpSafeHtml 的 v-html 已落地）。 */
async function markLinkStatus(): Promise<void> {
  statusSeq += 1;
  const seq = statusSeq;
  const relPath = props.relPath;
  if (!relPath) return;
  const index = await loadLinkStatus(relPath);
  if (!alive || seq !== statusSeq) return;
  await nextTick();
  if (!alive || seq !== statusSeq || !rootRef.value) return;
  applyLinkStatus(rootRef.value, index);
}

// **必须挂在 onMounted 里**：setup 阶段 `rootRef` 还是 null（v-html 也还没落地），
// 那时调用会直接返回 —— 于是"悬空链接的视觉区分"静默失效（实测：4 条用例红）。
onMounted(() => void markLinkStatus());
watch(() => [props.relPath, props.source], () => void markLinkStatus());

/** 该元素现在是否已被判定为悬空（点击时据此决定"跳转"还是"创建"）。 */
function isDangling(el: Element): boolean {
  return el.classList.contains('kp-link--dangling');
}

function handleActivate(event: Event): void {
  const target = event.target as HTMLElement | null;
  const remote = target?.closest('[data-kp-remote-src]');
  if (remote) {
    emit('remoteImage', remote.getAttribute('data-kp-remote-src') ?? '');
    return;
  }
  const tag = target?.closest('[data-kp-tag]');
  if (tag) {
    emit('tag', tag.getAttribute('data-kp-tag') ?? '');
    return;
  }
  const link = target?.closest('[data-kp-link]');
  if (!link) return;
  const mouse = event as MouseEvent;
  const withModifier = mouse.ctrlKey === true || mouse.metaKey === true;
  if (!withModifier && !isDangling(link)) return;
  emit(
    'link',
    {
      target: link.getAttribute('data-kp-link') ?? '',
      kind: link.getAttribute('data-kp-kind') ?? 'link',
      dangling: isDangling(link),
    },
    event instanceof MouseEvent ? event : undefined,
  );
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter' || event.key === ' ') {
    handleActivate(event);
  }
}
</script>

<template>
  <article ref="rootRef" class="kp-markdown" @click="handleActivate" @keydown="onKeydown">
    <KpSafeHtml :html="html" />
  </article>
</template>

<style scoped>
.kp-markdown { line-height: 1.7; word-break: break-word; }
.kp-markdown :deep(.kp-remote-image) { display: inline-block; padding: 2px 8px; border: 1px dashed currentcolor; border-radius: 4px; cursor: pointer; opacity: 0.8; }
.kp-markdown :deep(.kp-link) { color: #1565c0; text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
.kp-markdown :deep(.kp-link[data-kp-kind='embed']) { color: #6a1b9a; }
/* FR-EDITOR-27：悬空用虚线下划线 + 降饱和，与正常链接一眼可分 */
.kp-markdown :deep(.kp-link--dangling) { color: #9e9e9e; text-decoration-style: dashed; }
.kp-markdown :deep(.kp-link--ambiguous) { color: #b26a00; text-decoration-style: dotted; }
.kp-markdown :deep(.kp-link--dangling)::after { content: ' ⚠'; font-size: 0.8em; }
.kp-markdown :deep(.kp-tag) { color: #00695c; background: #e0f2f1; border-radius: 3px; padding: 0 4px; cursor: pointer; }
</style>
