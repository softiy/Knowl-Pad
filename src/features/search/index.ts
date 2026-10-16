/** 搜索域前端入口（FR-SEARCH-01~11 / 40 / 41）：面板、快速打开与它们的数据源。 */
export { default as SearchPanel } from './components/SearchPanel.vue';
export { default as QuickOpen } from './components/QuickOpen.vue';
export { useSearchStore, PAGE_SIZE } from './stores/search';
export * from './logic';
