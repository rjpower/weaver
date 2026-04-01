<script setup lang="ts">
import type { DiffResponse } from '../types'

defineProps<{
  data: DiffResponse | null
  loading: boolean
}>()

interface DiffLine {
  text: string
  type: string
}

function parseDiff(raw: string): DiffLine[] {
  if (!raw) return []
  return raw.split('\n').map(line => {
    let type = 'context'
    if (line.startsWith('+++') || line.startsWith('---')) type = 'file'
    else if (line.startsWith('@@')) type = 'hunk'
    else if (line.startsWith('+')) type = 'add'
    else if (line.startsWith('-')) type = 'del'
    return { text: line, type }
  })
}

function lineClass(type: string): string {
  switch (type) {
    case 'add': return 'bg-success/[0.08] text-success'
    case 'del': return 'bg-error/[0.08] text-error'
    case 'hunk': return 'text-info font-semibold bg-info/[0.06] py-1'
    case 'file': return 'text-text-primary font-bold py-2 bg-elevated border-b border-border'
    default: return ''
  }
}
</script>

<template>
  <div class="bg-surface border border-border rounded-lg mb-4">
    <div class="px-4 py-3.5 border-b border-border flex items-center justify-between">
      <span class="text-[13px] font-semibold text-text-primary">Changes</span>
      <span v-if="data?.branch" class="font-mono text-xs text-text-dim">{{ data.branch }} &larr; {{ data.base }}</span>
    </div>
    <div v-if="loading" class="p-4 text-text-dim text-[13px]">Loading diff...</div>
    <div v-else-if="data?.error" class="p-4 text-error text-[13px]">{{ data.error }}</div>
    <div v-else-if="data?.diff" class="font-mono text-xs leading-relaxed overflow-x-auto">
      <div v-for="(line, i) in parseDiff(data.diff)" :key="i"
           :class="['whitespace-pre px-3', lineClass(line.type)]">{{ line.text }}</div>
    </div>
    <div v-else class="p-4 text-text-dim text-[13px]">No changes found</div>
  </div>
</template>
