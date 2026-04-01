<script setup lang="ts">
import type { IssueStatus } from '../types'
import { statusClasses } from '../utils'

defineProps<{
  status: IssueStatus
}>()
</script>

<template>
  <span :class="['inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-semibold uppercase tracking-wide', statusClasses(status)]">
    <span v-if="status === 'running'" class="w-1.5 h-1.5 rounded-full bg-current animate-pulse-dot" />
    {{ status.replace('_', ' ') }}
  </span>
</template>
