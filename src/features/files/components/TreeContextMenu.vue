<script setup lang="ts">
import type { MenuAction } from '../types';

defineProps<{ x: number; y: number; isDir: boolean }>();
const emit = defineEmits<{ select: [action: MenuAction]; close: [] }>();
</script>

<template>
  <div class="kp-menu" :style="{ left: `${x}px`, top: `${y}px` }" @mouseleave="emit('close')">
    <button v-if="isDir" type="button" @click="emit('select', 'newNote')">新建笔记</button>
    <button v-if="isDir" type="button" @click="emit('select', 'newFolder')">新建文件夹</button>
    <button type="button" @click="emit('select', 'rename')">重命名</button>
    <button type="button" @click="emit('select', 'delete')">删除</button>
    <button type="button" @click="emit('select', 'reveal')">在文件管理器中显示</button>
    <button type="button" @click="emit('select', 'copyPath')">复制相对路径</button>
  </div>
</template>

<style scoped>
.kp-menu { position: fixed; z-index: 100; min-width: 160px; padding: 4px; background: #fff; border: 1px solid #ccc; border-radius: 4px; box-shadow: 0 4px 12px rgb(0 0 0 / 15%); display: flex; flex-direction: column; }
.kp-menu button { text-align: left; padding: 4px 8px; background: none; border: 0; cursor: pointer; font-size: 13px; }
.kp-menu button:hover { background: #f2f2f2; }
</style>
