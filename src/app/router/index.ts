import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router';

/** PRD §7.1 的路由表（M1/M2 复核补齐：V-WELCOME 此前缺失）。 */
const routes: RouteRecordRaw[] = [
  { path: '/welcome', name: 'welcome', component: () => import('@features/vault/components/VaultList.vue') },
  { path: '/', name: 'workspace', component: () => import('../layout/WorkspaceLayout.vue') },
];

export const router = createRouter({ history: createWebHashHistory(), routes });

/**
 * RT-01：除 /welcome 外，全部路由必须有打开的 Vault，否则重定向到 /welcome。
 * store 的初始化发生在守卫内（此时 pinia 已安装）。
 */
router.beforeEach(async (to) => {
  if (to.path === '/welcome') return true;
  const { useVaultStore } = await import('@features/vault');
  const vault = useVaultStore();
  if (vault.current) return true;
  await vault.restoreCurrent();
  return vault.current ? true : { path: '/welcome' };
});
