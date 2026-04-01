<script setup lang="ts">
import { useRouter } from 'vue-router'
import type { Issue, IssueStatus } from '../types'
import { truncate } from '../utils'
import StatusBadge from './StatusBadge.vue'

defineOptions({ name: 'DagNode' })

const props = defineProps<{
  node: { issue: Issue; children: { issue: Issue; children: any[] }[] }
  statusDotClass: Record<IssueStatus, string>
}>()

const router = useRouter()
</script>

<template>
  <div>
    <div class="flex items-center gap-2 py-1 text-[13px] cursor-pointer rounded px-1 -mx-1 hover:bg-hover transition-colors"
         @click="router.push('/issues/' + node.issue.id)">
      <span :class="[statusDotClass[node.issue.status], 'w-2 h-2 rounded-full shrink-0']"></span>
      <span class="font-mono text-xs text-text-dim">{{ node.issue.id }}</span>
      <span class="text-text-primary">{{ truncate(node.issue.title, 50) }}</span>
      <StatusBadge :status="node.issue.status" />
    </div>
    <div v-if="node.children.length" class="ml-3 border-l border-border/50 pl-3">
      <DagNode v-for="child in node.children" :key="child.issue.id" :node="child" :status-dot-class="statusDotClass" />
    </div>
  </div>
</template>
