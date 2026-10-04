<script setup lang="ts">
import { computed } from 'vue';
import { renderMarkdown } from '@core/markdown/renderer';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';

/**
 * 阅读态渲染（技术方案 §7.2 ④⑤）。
 *
 * - 渲染 → 净化 → 图片策略全在 core/markdown/renderer 内完成；
 * - 本组件是全项目**唯一**允许把渲染结果交给 v-html 的地方（经由 KpSafeHtml）；
 * - 事件用**容器级委托**（不逐个绑监听器）：读 data-kp-* 后向外抛事件，
 *   由上层决定路由/命令（远程图片只允许交给系统浏览器打开，SEC-08）。
 */
const props = defineProps<{
  source: string;
  resolveAsset?: (src: string) => string | null;
  cacheScope?: string;
}>();

const emit = defineEmits<{
  /** 点击远程图片占位符（上层应交给系统浏览器打开，应用内不加载） */
  remoteImage: [src: string];
  /** 点击带 data-kp-link 的内部链接（M4 接路由跳转） */
  link: [href: string];
}>();

const html = computed(() =>
  renderMarkdown(props.source, {
    resolveAsset: props.resolveAsset,
    cacheScope: props.cacheScope,
  }),
);

function handleActivate(event: Event): void {
  const target = event.target as HTMLElement | null;
  const remote = target?.closest('[data-kp-remote-src]');
  if (remote) {
    emit('remoteImage', remote.getAttribute('data-kp-remote-src') ?? '');
    return;
  }
  const link = target?.closest('[data-kp-link]');
  if (link) {
    emit('link', link.getAttribute('data-kp-link') ?? '');
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter' || event.key === ' ') {
    handleActivate(event);
  }
}
</script>

<template>
  <article class="kp-markdown" @click="handleActivate" @keydown="onKeydown">
    <KpSafeHtml :html="html" />
  </article>
</template>

<style scoped>
.kp-markdown { line-height: 1.7; word-break: break-word; }
.kp-markdown :deep(.kp-remote-image) { display: inline-block; padding: 2px 8px; border: 1px dashed currentcolor; border-radius: 4px; cursor: pointer; opacity: 0.8; }
</style>
