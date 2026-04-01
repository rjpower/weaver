<script setup lang="ts">
import type { Issue } from '../types'
import StatusBadge from './StatusBadge.vue'
import TagEditor from './TagEditor.vue'

defineProps<{
  issue: Issue
}>()

const emit = defineEmits<{
  'update:tags': [tags: string[]]
}>()
</script>

<template>
  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-text-secondary mb-4 px-1">
    <TagEditor :tags="issue.tags || []" :issue-id="issue.id" @update="tags => emit('update:tags', tags)" />
    <span class="text-text-dim">&middot;</span>
    <span class="font-mono">try {{ issue.num_tries || 0 }}/{{ issue.max_tries || 3 }}</span>
    <template v-if="issue.priority">
      <span class="text-text-dim">&middot;</span>
      <span class="font-mono">p{{ issue.priority }}</span>
    </template>
    <template v-if="issue.usage">
      <span class="text-text-dim">&middot;</span>
      <span class="font-mono">{{ issue.usage.input_tokens.toLocaleString() }}in / {{ issue.usage.output_tokens.toLocaleString() }}out</span>
      <template v-if="issue.usage.cost_usd">
        <span class="text-text-dim">&middot;</span>
        <span class="font-mono">${{ issue.usage.cost_usd.toFixed(4) }}</span>
      </template>
      <template v-if="issue.usage.model">
        <span class="text-text-dim">&middot;</span>
        <span class="font-mono text-text-dim">{{ issue.usage.model }}</span>
      </template>
    </template>
  </div>
</template>
