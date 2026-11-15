<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { renderMarkdown } from '@core/markdown/renderer';
import { applyLinkStatus, dstRelPathOf, loadLinkStatus, type LinkStatusIndex } from '@core/markdown/linkStatus';
import { openExternal } from '@core/ipc/externalCommands';
import { asKpError } from '@core/ipc/errors';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';

/**
 * 阅读态渲染（技术方案 §7.2 ④⑤；FR-EDITOR-02/25/27、FR-TAG-05）。
 *
 * - 渲染 → 净化 → 图片策略全在 core/markdown/renderer 内完成；扩展语法（`[[x]]`/`#标签`）
 *   也在那里变成带 `data-kp-*` 的中转节点；
 * - 本组件是全项目**唯一**允许把渲染结果交给 v-html 的地方（经由 KpSafeHtml）；
 * - 事件用**容器级委托**（不逐个绑监听器）：读 `data-kp-*` 后向外抛事件；
 *   远程图片占位符的"打开"在本组件内直接走 `core/ipc/openExternal`（只交给系统浏览器，SEC-08），
 *   其余（链接/标签）由上层决定路由；
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
  resolveAsset?: ((src: string) => string | null) | undefined;
  cacheScope?: string;
}>();

const emit = defineEmits<{
  /** 用户点击了远程图片占位符（应用内不加载；打开动作由本组件直接走 `openExternal`，emit 保留供上层/测试观测） */
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
/**
 * 最近一次链接裁决（点击跳转要用 `dstRelPath`，见 `handleActivate`）。
 *
 * 为什么不从 DOM 上再读一遍：DOM 只带**显示口径**的 `data-kp-link`，真实路径只存在于
 * `link_outgoing` 的结果里；把整份索引记在这里，点击时按规范化目标查一次即可。
 */
let linkIndex: LinkStatusIndex = { byTarget: new Map() };

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
  linkIndex = index;
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

/**
 * 远程图片占位符被激活（点击 / 回车，E2）：**交给系统默认浏览器**，应用进程不加载（SEC-08）。
 *
 * 为什么在组件里直接调 IPC 封装而不是 emit 给上层：占位符的语义是"打开这个 URL"，
 * 与上层（文件树/标签域）无关；走 `core/ipc` 只依赖 core 层，不违反 FE-02。
 * 打开失败必须可见 —— 这里降到 console.warn（与 adapter 的降级提示同一口径，R-15）。
 */
function activateRemoteImage(src: string): void {
  if (!src) return;
  emit('remoteImage', src);
  void openExternal(src).catch((err: unknown) => {
    console.warn(`打开远程图片失败：${asKpError(err).message}`);
  });
}

/**
 * 点击时的**真实跳转目标**（D2 / 10.7）：已裁决链接换成 `dstRelPath`，其余保持显示目标。
 *
 * 为什么不能一律用显示目标：`[[B]]` / `[[B|别名]]` 的 `data-kp-link` 都是"B"（显示口径），
 * 而上层（`editor.reveal` / `openNoteInNewTab`）只认识 Vault 相对路径 —— 把 "B" 交给
 * `note_read` 会报「文件不存在」（M5 真机 D2 的根因）。显示文本不动，只换事件载荷。
 * 悬空 / 歧义 / 索引未就绪时**不猜**路径，仍发原目标（创建流程用显示名，跳过失败时由上层提示）。
 */
function linkTargetOf(link: Element, dangling: boolean): string {
  const displayTarget = link.getAttribute('data-kp-link') ?? '';
  if (dangling) return displayTarget;
  return dstRelPathOf(linkIndex, displayTarget) ?? displayTarget;
}

function handleActivate(event: Event): void {
  const target = event.target as HTMLElement | null;
  const remote = target?.closest('[data-kp-remote-src]');
  if (remote) {
    activateRemoteImage(remote.getAttribute('data-kp-remote-src') ?? '');
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
  const dangling = isDangling(link);
  if (!withModifier && !dangling) return;
  emit(
    'link',
    {
      target: linkTargetOf(link, dangling),
      kind: link.getAttribute('data-kp-kind') ?? 'link',
      dangling,
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
.kp-markdown :deep(.kp-link) { color: var(--kp-hl-link); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
.kp-markdown :deep(.kp-link[data-kp-kind='embed']) { color: var(--kp-hl-keyword); }
/* FR-EDITOR-27：悬空用虚线下划线 + 降饱和，与正常链接一眼可分 */
.kp-markdown :deep(.kp-link--dangling) { color: var(--kp-muted); text-decoration-style: dashed; }
.kp-markdown :deep(.kp-link--ambiguous) { color: var(--kp-hl-number); text-decoration-style: dotted; }
.kp-markdown :deep(.kp-link--dangling)::after { content: ' ⚠'; font-size: 0.8em; }
.kp-markdown :deep(.kp-tag) { color: var(--kp-hl-string); background: var(--kp-accent-weak); border-radius: 3px; padding: 0 4px; cursor: pointer; }

/* ── 代码块高亮（FR-EDITOR-44 / DEBT-18）：class 全部是 kp-hl-*，沿用上面的浅色主题 ── */
.kp-markdown :deep(pre.kp-pre) { background: var(--kp-code-bg); color: var(--kp-fg); border: 1px solid var(--kp-border, #eee); border-radius: 4px; padding: 8px 10px; overflow: auto; }
.kp-markdown :deep(code.kp-hl) { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: var(--kp-editor-font-size, 13px); line-height: var(--kp-editor-line-height, 1.5); }
.kp-markdown :deep(.kp-hl-keyword) { color: var(--kp-hl-keyword); font-weight: 600; }
.kp-markdown :deep(.kp-hl-string) { color: var(--kp-hl-string); }
.kp-markdown :deep(.kp-hl-comment) { color: var(--kp-hl-comment); font-style: italic; }
.kp-markdown :deep(.kp-hl-number) { color: var(--kp-hl-number); }
.kp-markdown :deep(.kp-hl-literal) { color: var(--kp-hl-literal); }
.kp-markdown :deep(.kp-hl-var) { color: var(--kp-hl-var); }
.kp-markdown :deep(.kp-hl-heading) { color: var(--kp-hl-heading); font-weight: 600; }
.kp-markdown :deep(.kp-hl-quote) { color: var(--kp-hl-comment); font-style: italic; }
.kp-markdown :deep(.kp-hl-list) { color: var(--kp-hl-keyword); }
.kp-markdown :deep(.kp-hl-code) { color: var(--kp-hl-code); }
.kp-markdown :deep(.kp-hl-link) { color: var(--kp-hl-link); }
.kp-markdown :deep(.kp-hl-url) { color: var(--kp-hl-url); text-decoration: underline; }
.kp-markdown :deep(.kp-hl-strong) { font-weight: 700; }
.kp-markdown :deep(.kp-hl-em) { font-style: italic; }
</style>
