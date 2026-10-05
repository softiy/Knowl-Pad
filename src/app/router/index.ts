import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router';

const routes: RouteRecordRaw[] = [
  // 工作区：左文件树 + 右编辑器；未打开 Vault 时布局内部展示 Vault 列表
  { path: '/', name: 'workspace', component: () => import('@/app/layout/WorkspaceLayout.vue') },
];

export const router = createRouter({ history: createWebHashHistory(), routes });
