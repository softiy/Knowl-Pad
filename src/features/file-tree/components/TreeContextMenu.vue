<script setup lang="ts">
import { computed } from 'vue';
import type { MenuAction } from '../types';

const props = defineProps<{ x: number; y: number; isDir: boolean; name: string }>();

/**
 * FR-STORAGE-02：第三方配置目录（.obsidian/.git）与应用内部目录（.knowlpad）
 * **不得**被新建/重命名/删除。后端（PathGuard + file_ops）已硬性拒绝，
 * 这里同步把入口藏掉——可见（AC-FILE-09）≠ 可写。
 */
const PROTECTED = ['.obsidian', '.git', '.knowlpad'];
const isProtected = computed(() => PROTECTED.some((p) => p.toLowerCase() === props.name.toLowerCase()));
const emit = defineEmits<{ select: [action: MenuAction]; close: [] }>();
</script>

<template>
  <div class="kp-menu" :style="{ left: `${x}px`, top: `${y}px` }" @mouseleave="emit('close')">
    <button v-if="isDir && !isProtected" type="button" @click="emit('select', 'newNote')">新建笔记</button>
    <button v-if="isDir && !isProtected" type="button" @click="emit('select', 'newFolder')">新建文件夹</button>
    <button v-if="!isProtected" type="button" @click="emit('select', 'rename')">重命名</button>
    <button v-if="!isProtected" type="button" @click="emit('select', 'delete')">删除</button>
    <p v-if="isProtected" class="kp-menu__note" data-testid="protected-note">
      Knowl Pad 不会修改该目录的内容
    </p>
    <button type="button" @click="emit('select', 'reveal')">在文件管理器中显示</button>
    <button type="button" @click="emit('select', 'copyPath')">复制相对路径</button>
  </div>
</template>

<style scoped>
.kp-menu { position: fixed; z-index: 100; min-width: 160px; padding: 4px; background: #fff; border: 1px solid #ccc; border-radius: 4px; box-shadow: 0 4px 12px rgb(0 0 0 / 15%); display: flex; flex-direction: column; }
.kp-menu button { text-align: left; padding: 4px 8px; background: none; border: 0; cursor: pointer; font-size: 13px; }
.kp-menu button:hover { background: #f2f2f2; }
.kp-menu__note { margin: 2px 0 0; padding: 4px 8px; color: #888; font-size: 12px; }
</style>
