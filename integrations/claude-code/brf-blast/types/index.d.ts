export type BrfPane = { block: string; root: string }

declare module 'claude-code' {
  interface PluginState {
    'brf-blast': { pane: BrfPane | null }
  }
}
