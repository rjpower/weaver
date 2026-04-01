<script setup lang="ts">
import { computed } from 'vue'
import type { Issue, IssueStatus } from '../types'
import { truncate } from '../utils'
import StatusBadge from './StatusBadge.vue'
import DagNode from './DagNode.vue'

const props = defineProps<{
  root: Issue
  descendants: Issue[]
}>()

interface TreeNode {
  issue: Issue
  children: TreeNode[]
}

const tree = computed<TreeNode>(() => {
  const childMap = new Map<string, Issue[]>()
  for (const d of props.descendants) {
    const pid = d.parent_issue_id ?? ''
    if (!childMap.has(pid)) childMap.set(pid, [])
    childMap.get(pid)!.push(d)
  }
  function build(issue: Issue): TreeNode {
    const kids = childMap.get(issue.id) ?? []
    return { issue, children: kids.map(build) }
  }
  return build(props.root)
})

const statusDotClass: Record<IssueStatus, string> = {
  pending: 'bg-warning',
  running: 'bg-info',
  completed: 'bg-success',
  failed: 'bg-error',
  validation_failed: 'bg-error',
  blocked: 'bg-warning',
  awaiting_review: 'bg-accent',
}
</script>

<template>
  <div class="bg-surface border border-border rounded-lg mb-4">
    <div class="px-4 py-3.5 border-b border-border">
      <span class="text-[13px] font-semibold text-text-primary">Issue Tree</span>
    </div>
    <div class="p-4">
      <!-- Root node -->
      <div class="flex items-center gap-2 py-1 text-[13px]">
        <span :class="[statusDotClass[tree.issue.status], 'w-2 h-2 rounded-full shrink-0']"></span>
        <span class="font-mono text-xs text-text-dim">{{ tree.issue.id }}</span>
        <span class="text-text-primary font-medium">{{ truncate(tree.issue.title, 50) }}</span>
        <StatusBadge :status="tree.issue.status" />
      </div>
      <!-- Children (recursive) -->
      <div v-if="tree.children.length" class="ml-3 border-l border-border/50 pl-3 mt-1">
        <template v-for="child in tree.children" :key="child.issue.id">
          <DagNode :node="child" :status-dot-class="statusDotClass" />
        </template>
      </div>
    </div>
  </div>
</template>
