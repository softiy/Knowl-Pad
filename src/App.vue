<script setup lang="ts">
import { ref } from 'vue';
import KpSafeHtml from '@shared/components/KpSafeHtml.vue';

const raw = ref<string>('<p>Knowl Pad <strong>Markdown</strong> 渲染</p>');
</script>

<template>
  <main>
    <h1>Knowl Pad</h1>
    <KpSafeHtml :html="raw" />
    <router-view />
  </main>
</template>
