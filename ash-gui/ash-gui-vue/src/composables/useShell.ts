/**
 * useShell — the entry point composable. Plan 042 M4: auto-selects the backend
 * transport based on environment:
 *
 *   - Tauri (`__TAURI_INTERNALS__` present) → useShellTauri (Tauri IPC)
 *   - Browser (no Tauri)                    → useShellHttp (fetch + SSE)
 *
 * Both return the SAME shape, so components don't change. useShellMock is gone
 * — the browser version now connects to the real Shell engine via ash-server.
 */
import { useShellTauri } from './useShellTauri'
import { useShellHttp } from './useShellHttp'

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export function useShell() {
  return isTauri ? useShellTauri() : useShellHttp()
}
