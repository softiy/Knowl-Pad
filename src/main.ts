import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import './core/theme/tokens.css';
import { router } from './app/router';
import { bootstrap } from './app/bootstrap';

/**
 * 先建 pinia/app，再等外观初始化完成后才 mount（顺序见 `app/bootstrap.ts` 的注释）：
 * 设置（主题/字号/缩放）必须在首次渲染前 apply，否则会闪默认主题、设置面板先显示默认值。
 */
const pinia = createPinia();
const app = createApp(App).use(pinia).use(router);

bootstrap(pinia)
  .then(() => app.mount('#app'))
  .catch((err: unknown) => {
    // R-15：不静默吞——自检失败要留在可诊断的日志里；且**不 mount**（AC-SEC-01 阻断启动）。
    console.warn('启动初始化失败，已阻断应用启动', err instanceof Error ? err.message : String(err));
  });
