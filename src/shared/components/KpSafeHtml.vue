<script setup lang="ts">
import { computed } from 'vue';
import { sanitizeHtml } from '@core/markdown/sanitize';

const props = defineProps<{ html: string }>();
// 唯一允许 v-html 的位置；内容必须先经 sanitizeHtml 净化（IP-01 / SEC-01）
const safe = computed<string>(() => sanitizeHtml(props.html));
</script>

<template>
  <div class="kp-safe-html" data-kp-sanitized="true" v-html="safe" />
</template>
