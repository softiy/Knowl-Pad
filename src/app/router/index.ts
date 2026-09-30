import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router';

const routes: RouteRecordRaw[] = [
  { path: '/', name: 'workspace', component: () => import('@features/vault/components/VaultList.vue') },
];

export const router = createRouter({ history: createWebHashHistory(), routes });
