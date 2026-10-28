export { COMMANDS, disabledReason, isEnabled, EDITOR_REASON, VAULT_REASON } from './registry';
export type { PaletteActions, PaletteCommand, PaletteContext, PaletteGroup } from './registry';
export { createCommandIndex, searchCommands, PAGE_SIZE } from './search';
export { usePaletteStore } from './store';
export { registerPaletteShortcut } from './shortcut';
export { default as CommandPalette } from './components/CommandPalette.vue';
