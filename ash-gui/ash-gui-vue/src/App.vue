<script setup lang="ts">
/**
 * App — the root layout: optional tool sidebar, a top title bar (cwd
 * breadcrumb), a scrollable Block list, and the bottom PromptBar.
 *
 * In a browser (`npm run dev` without Tauri) we use a mock backend so the UI
 * is previewable; inside Tauri the real backend is used. The two composables
 * return identical shapes, so nothing else changes.
 */
import { ref, computed } from 'vue'
import { useShell } from '@/composables/useShell'
import BlockList from '@/components/block/BlockList.vue'
import PromptBar from '@/components/input/PromptBar.vue'
import ToolSidebar from '@/components/chrome/ToolSidebar.vue'

// Plan 042 M4: useShell() auto-selects Tauri IPC or HTTP (ash-server) based on
// environment. No more useShellMock — both versions connect to the real engine.
const {
  blocks,
  cwd,
  home,
  commands,
  smartCommands,
  commandNames,
  history,
  gitInfo,
  runCommand,
  runSmartCommand,
  cancelCommand,
  complete,
  openPath,
} = useShell()

const sidebarOpen = ref(true)
/** Command injected from the sidebar into the PromptBar input. */
const injectedCommand = ref('')

function onOpenPath(path: string) {
  void openPath(path)
}

function onRerun(command: string) {
  void runCommand(command)
}

function onPickTool(name: string) {
  injectedCommand.value = name
}

function onInjected() {
  // PromptBar consumed the injected command.
  injectedCommand.value = ''
}

/** Plan 040 M3: a SmartCommand was clicked — run it by name. */
function onRunSmart(name: string) {
  void runSmartCommand(name)
}

/** Plan 040 M5: cancel the running command (stop button on a Running block). */
function onStop() {
  void cancelCommand()
}

/** Plan 041 M6: Ctrl+L — clear the screen (archive all blocks). */
function onClear() {
  blocks.splice(0, blocks.length)
}

/** Plan 041 M6: Ctrl+D on empty input — exit the app. */
function onExit() {
  window.close()
}

/** Plan 041 M5: format the git status as +N !N ?N ⇡N ⇣N (like the TUI prompt). */
const gitLabel = computed(() => {
  const g = gitInfo.value
  if (!g.git_branch) return ''
  const s = g.git_status
  let label = `⎇ ${g.git_branch}`
  if (s) {
    const parts: string[] = []
    if (s.staged) parts.push(`+${s.staged}`)
    if (s.unstaged) parts.push(`!${s.unstaged}`)
    if (s.untracked) parts.push(`?${s.untracked}`)
    if (s.conflicted) parts.push(`✗${s.conflicted}`)
    if (s.ahead) parts.push(`⇡${s.ahead}`)
    if (s.behind) parts.push(`⇣${s.behind}`)
    if (parts.length) label += ' ' + parts.join(' ')
  }
  return label
})
</script>

<template>
  <div class="flex h-full bg-background">
    <!-- Tool sidebar (optional) -->
    <ToolSidebar
      v-if="sidebarOpen"
      :commands="commands"
      :smart-commands="smartCommands"
      @pick="onPickTool"
      @run-smart="onRunSmart"
    />

    <!-- Main column -->
    <div class="flex-1 flex flex-col min-w-0">
      <!-- Title bar: sidebar toggle + cwd -->
      <header class="flex items-center gap-2 px-3 h-9 border-b border-border bg-card/40 shrink-0">
        <button
          class="text-xs px-1.5 py-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors"
          title="Toggle tool sidebar"
          @click="sidebarOpen = !sidebarOpen"
        >
          🛠
        </button>
        <span class="text-sm font-semibold text-foreground/90">ash</span>
        <span class="text-muted-foreground/40">·</span>
        <span class="text-xs font-mono-ash text-sky-300/80 truncate" :title="cwd">
          {{ cwd ? cwd.replace(/\\/g, '/') : '…' }}
        </span>
        <span v-if="gitLabel" class="text-xs font-mono-ash text-amber-400/80 shrink-0">
          {{ gitLabel }}
        </span>
      </header>

      <!-- Block list (scrollable) -->
      <BlockList
        :blocks="blocks"
        :home="home"
        @open-path="onOpenPath"
        @rerun="onRerun"
        @stop="onStop"
      />

      <!-- Command input -->
      <PromptBar
        :cwd="cwd"
        :home="home"
        :command-names="commandNames"
        :history="history"
        :injected-command="injectedCommand"
        :complete="complete"
        @run="runCommand($event)"
        @injected="onInjected"
        @clear="onClear"
        @exit="onExit"
      />
    </div>
  </div>
</template>
