<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'
import { relativeTime, truncate, duration, organizeByParent } from '../utils'
import { usePolling } from '../composables/usePolling'
import type { Issue, IssueListResponse, OrganizedIssue } from '../types'
import StatusBadge from '../components/StatusBadge.vue'
import Pagination from '../components/Pagination.vue'
import IssueForm from '../components/IssueForm.vue'

const router = useRouter()
const issues = ref<Issue[]>([])
const total = ref(0)
const loading = ref(true)
const statusFilter = ref('')
const limit = 25
const offset = ref(0)
const showNewIssue = ref(false)

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / limit)))
const currentPage = computed(() => Math.floor(offset.value / limit) + 1)
const organized = computed(() => organizeByParent(issues.value))

function hasActive(): boolean {
  return issues.value.some(i => i.status === 'running' || i.status === 'pending' || i.status === 'awaiting_review')
}

async function load() {
  try {
    const params = new URLSearchParams()
    if (statusFilter.value) params.set('status', statusFilter.value)
    params.set('limit', String(limit))
    params.set('offset', String(offset.value))
    const data = await api<IssueListResponse>('/api/issues?' + params.toString())
    issues.value = data.issues
    total.value = data.total
    markUpdated()
  } finally {
    loading.value = false
  }
}

const { now, markUpdated, lastUpdatedAgo } = usePolling(hasActive, load, 5000)

function liveDuration(issue: OrganizedIssue): string {
  if (issue.status === 'running') return duration(issue.created_at, null)
  return duration(issue.created_at, issue.completed_at)
}

function goPage(page: number) {
  offset.value = (page - 1) * limit
  load()
}

watch(statusFilter, () => {
  offset.value = 0
  load()
})

load()
</script>

<template>
  <div class="px-8 py-7 max-md:px-4 max-w-[1400px]">
    <div class="flex items-center justify-between mb-6">
      <h2 class="text-xl font-bold text-text-primary tracking-tight">Issues</h2>
      <div class="flex items-center gap-3">
        <span class="font-mono text-xs text-text-secondary">{{ total }} total</span>
        <span class="text-[11px] text-text-dim">Updated {{ lastUpdatedAgo() }}</span>
        <button v-if="!showNewIssue" @click="showNewIssue = true"
                class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer transition-colors bg-accent text-white hover:bg-accent-hover">
          New Issue
        </button>
      </div>
    </div>

    <IssueForm v-if="showNewIssue" @close="showNewIssue = false" />

    <div class="flex gap-2 mb-4 items-center">
      <select v-model="statusFilter"
              class="w-[150px] py-1.5 px-2.5 text-[13px] text-text-primary bg-input border border-border rounded-md outline-none cursor-pointer appearance-none bg-no-repeat bg-[right_10px_center] pr-7 transition-colors focus:border-border-focus"
              style="background-image: url(&quot;data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%234a5468' stroke-width='2'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E&quot;);">
        <option value="">All statuses</option>
        <option value="pending">Pending</option>
        <option value="running">Running</option>
        <option value="completed">Completed</option>
        <option value="failed">Failed</option>
        <option value="blocked">Blocked</option>
        <option value="awaiting_review">Awaiting Review</option>
      </select>
    </div>

    <div class="bg-surface border border-border rounded-lg">
      <div v-if="loading" class="text-text-dim text-[13px] text-center py-8">Loading...</div>

      <div v-else-if="organized.length === 0" class="flex flex-col items-center justify-center py-16 px-8 text-center">
        <svg class="w-12 h-12 text-text-dim mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z"/>
        </svg>
        <div class="text-base font-semibold text-text-primary mb-2">No issues yet</div>
        <div class="text-[13px] text-text-secondary max-w-[360px] leading-relaxed">Create your first issue to get started. Issues represent tasks for the agent to work on.</div>
        <button @click="showNewIssue = true"
                class="mt-4 inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer transition-colors bg-accent text-white hover:bg-accent-hover">
          Create an Issue
        </button>
      </div>

      <div v-else class="overflow-x-auto">
        <table class="w-full border-collapse text-[13px]">
          <thead>
            <tr>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Status</th>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">ID</th>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Title</th>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Tags</th>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Created</th>
              <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Duration</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="issue in organized" :key="issue.id"
                @click="router.push('/issues/' + issue.id)"
                class="cursor-pointer transition-colors hover:bg-hover">
              <td class="px-4 py-2.5 border-b border-border text-text-secondary"
                  :style="issue._depth ? { paddingLeft: (16 + issue._depth * 20) + 'px' } : {}">
                <StatusBadge :status="issue.status" />
              </td>
              <td class="px-4 py-2.5 border-b border-border font-mono text-xs text-text-dim relative"
                  :style="issue._depth ? { paddingLeft: (16 + issue._depth * 20) + 'px' } : {}">
                <span v-if="issue._isChild" class="absolute text-text-dim font-mono"
                      :style="{ left: (issue._depth * 20 - 2) + 'px' }">&boxur;</span>
                {{ issue.id }}
              </td>
              <td class="px-4 py-2.5 border-b border-border text-text-primary">{{ truncate(issue.title, 80) }}</td>
              <td class="px-4 py-2.5 border-b border-border">
                <div class="flex flex-wrap gap-1">
                  <span v-for="tag in (issue.tags || [])" :key="tag" class="tag-chip-sm">{{ tag }}</span>
                  <span v-if="!(issue.tags || []).length" class="text-text-dim font-mono text-xs">&mdash;</span>
                </div>
              </td>
              <td class="px-4 py-2.5 border-b border-border whitespace-nowrap">{{ relativeTime(issue.created_at) }}</td>
              <td :class="['px-4 py-2.5 border-b border-border whitespace-nowrap', issue.status === 'running' ? 'text-info' : '']">
                {{ liveDuration(issue) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <Pagination :current-page="currentPage" :total-pages="totalPages" @page="goPage" />
    </div>
  </div>
</template>

<style scoped>
.tag-chip-sm {
  display: inline-flex;
  align-items: center;
  padding: 0 6px;
  border-radius: 9999px;
  background: var(--color-elevated);
  border: 1px solid var(--color-border);
  color: var(--color-text-secondary);
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 16px;
  white-space: nowrap;
}
</style>
