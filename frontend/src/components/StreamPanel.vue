<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue'
import type { StreamEvent } from '../types'

const props = defineProps<{
  events: StreamEvent[]
  connected: boolean
}>()

const container = ref<HTMLElement | null>(null)
const userScrolled = ref(false)
const collapsed = ref(false)

const displayEvents = computed(() =>
  props.events.filter(e => e.kind !== 'init')
)

function onScroll() {
  if (!container.value) return
  const el = container.value
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40
  userScrolled.value = !atBottom
}

watch(() => props.events.length, () => {
  if (!userScrolled.value && container.value) {
    nextTick(() => {
      container.value?.scrollTo({ top: container.value.scrollHeight })
    })
  }
})

function toolLabel(tool: string): string {
  return tool.replace(/^mcp__\w+__/, '')
}

function previewLines(text: string, maxLines: number = 3): string {
  const lines = text.split('\n')
  if (lines.length <= maxLines) return text
  return lines.slice(0, maxLines).join('\n') + '\n...'
}
</script>

<template>
  <div class="border border-border rounded-lg mb-3 overflow-hidden">
    <button @click="collapsed = !collapsed"
            class="w-full flex items-center justify-between px-3 py-2 bg-transparent border-none cursor-pointer text-left hover:bg-hover transition-colors">
      <span class="flex items-center gap-2">
        <span class="text-text-dim text-xs">{{ collapsed ? '\u25B8' : '\u25BE' }}</span>
        <span class="text-xs font-medium text-text-secondary">Live Output</span>
      </span>
      <span v-if="connected" class="flex items-center gap-1.5 text-[11px] text-success">
        <span class="w-1.5 h-1.5 rounded-full bg-success animate-pulse-dot"></span>
        Streaming
      </span>
    </button>
    <div v-show="!collapsed" ref="container" @scroll="onScroll"
         class="px-3 py-2 font-mono text-xs max-h-[500px] overflow-y-auto bg-body leading-relaxed">
      <template v-for="(event, i) in displayEvents" :key="i">
        <div v-if="event.kind === 'text'" class="mb-1.5">
          <span class="text-text-primary whitespace-pre-wrap break-words">{{ event.text }}</span>
        </div>
        <div v-else-if="event.kind === 'tool_use'" class="mb-1 flex items-start gap-2 text-[11px]">
          <span class="text-accent font-semibold shrink-0">{{ toolLabel(event.tool || '') }}</span>
          <span v-if="event.input" class="text-text-dim truncate">{{ previewLines(event.input, 1) }}</span>
        </div>
        <div v-else-if="event.kind === 'tool_result'" class="mb-1 text-[11px] text-text-secondary pl-4">
          <span class="whitespace-pre-wrap">{{ previewLines(event.output || '', 3) }}</span>
        </div>
        <div v-else-if="event.kind === 'error'" class="mb-1.5 text-error text-[11px]">
          {{ event.message }}
        </div>
        <div v-else-if="event.kind === 'result'" class="mb-1.5 pt-1.5 border-t border-border">
          <span class="text-[10px] font-semibold text-success uppercase tracking-wider">Completed</span>
        </div>
      </template>
      <div v-if="connected && !displayEvents.length" class="text-text-dim text-[11px]">
        Waiting for output...
      </div>
    </div>
  </div>
</template>
