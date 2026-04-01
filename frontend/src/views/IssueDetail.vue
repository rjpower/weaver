<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api, apiPost } from '../api'
import { relativeTime, duration, truncate } from '../utils'
import { usePolling } from '../composables/usePolling'
import { useEventStream } from '../composables/useEventStream'
import type { Issue, Comment, DiffResponse, PrInfoResponse } from '../types'
import StatusBadge from '../components/StatusBadge.vue'
import MetaGrid from '../components/MetaGrid.vue'
import JsonTree from '../components/JsonTree.vue'
import DiffViewer from '../components/DiffViewer.vue'
import FileViewer from '../components/FileViewer.vue'
import ReviewBanner from '../components/ReviewBanner.vue'
import CommentSection from '../components/CommentSection.vue'
import DagTree from '../components/DagTree.vue'
import StreamPanel from '../components/StreamPanel.vue'

const route = useRoute()
const router = useRouter()
const issue = ref<Issue | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
const cancelling = ref(false)
const comments = ref<Comment[]>([])
const childIssues = ref<Issue[]>([])

const reviewTab = ref('diff')
const diffData = ref<DiffResponse | null>(null)
const diffLoading = ref(false)
const diffLoaded = ref(false)
const reviewComment = ref('')
const approving = ref(false)
const requestingChanges = ref(false)
const prInfo = ref<PrInfoResponse | null>(null)

const treeDescendants = ref<Issue[]>([])

const showContext = ref(false)
const showReviseForm = ref(false)
const reviseFeedback = ref('')
const reviseSubmitting = ref(false)

const issueId = computed(() => route.params.id as string)
const isRunning = computed(() => issue.value?.status === 'running')
const { events: streamEvents, connected: streamConnected } = useEventStream(issueId, isRunning)

const resultComment = computed(() =>
  comments.value?.filter(c => c.tag === 'result').at(-1)
)
const canReview = computed(() => issue.value?.status === 'awaiting_review')
const hasWorktree = computed(() => issue.value?.context?.work_dir)
const canCancel = computed(() => issue.value && (issue.value.status === 'pending' || issue.value.status === 'running'))
const canRevise = computed(() => issue.value && ['completed', 'failed', 'validation_failed'].includes(issue.value.status))
const showTabs = computed(() => hasWorktree.value && (canReview.value || issue.value?.status === 'completed'))

function isActive(): boolean {
  if (!issue.value) return false
  return issue.value.status === 'running' || issue.value.status === 'pending' || issue.value.status === 'awaiting_review'
}

async function load() {
  try {
    const data = await api<Issue>('/api/issues/' + issueId.value)
    if (JSON.stringify(data) !== JSON.stringify(issue.value)) {
      issue.value = data
    }
    error.value = null
    markUpdated()
    if (data.context?.work_dir) {
      loadPrInfo()
    }
    if ((data.status === 'awaiting_review' || data.status === 'completed') && data.context?.work_dir && !diffLoaded.value) {
      loadDiff()
      diffLoaded.value = true
    }
  } catch {
    error.value = 'Issue not found'
  } finally {
    loading.value = false
  }
}

async function loadComments() {
  try {
    const data = await api<Comment[]>('/api/issues/' + issueId.value + '/comments')
    const incoming = Array.isArray(data) ? data : []
    if (incoming.length !== comments.value.length ||
        JSON.stringify(incoming) !== JSON.stringify(comments.value)) {
      comments.value = incoming
    }
  } catch {
    comments.value = []
  }
}

async function loadChildren() {
  try {
    const data = await api<{ issues: Issue[] }>('/api/issues?parent_issue_id=' + issueId.value + '&limit=50')
    childIssues.value = data.issues || []
  } catch {
    childIssues.value = []
  }
}

async function loadPrInfo() {
  try {
    prInfo.value = await api<PrInfoResponse>('/api/issues/' + issueId.value + '/pr')
  } catch {
    prInfo.value = null
  }
}

async function loadTree() {
  try {
    const data = await api<Issue[]>('/api/issues/' + issueId.value + '/tree')
    treeDescendants.value = Array.isArray(data) ? data : []
  } catch {
    treeDescendants.value = []
  }
}

async function loadDiff() {
  if (!hasWorktree.value) return
  diffLoading.value = true
  try {
    diffData.value = await api<DiffResponse>('/api/issues/' + issueId.value + '/diff')
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    diffData.value = { diff: '', files_changed: [], error: msg, branch: null, base: '', work_dir: null }
  } finally {
    diffLoading.value = false
  }
}

async function cancelIssue() {
  if (!confirm('Cancel this issue?')) return
  cancelling.value = true
  try {
    const updated = await apiPost<Issue>('/api/issues/' + issueId.value + '/cancel')
    if (updated) issue.value = updated
    else await load()
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    alert('Failed to cancel issue: ' + msg)
  } finally {
    cancelling.value = false
  }
}

async function approveIssue() {
  approving.value = true
  try {
    const payload: Record<string, unknown> = {}
    if (reviewComment.value.trim()) payload.comment = reviewComment.value
    const updated = await apiPost<Issue>('/api/issues/' + issueId.value + '/approve', payload)
    if (updated) issue.value = updated
    else await load()
    reviewComment.value = ''
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    alert('Failed to approve: ' + msg)
  } finally {
    approving.value = false
  }
}

async function requestChanges() {
  if (!reviewComment.value.trim()) {
    alert('Please provide feedback for the requested changes.')
    return
  }
  requestingChanges.value = true
  try {
    const updated = await apiPost<Issue>('/api/issues/' + issueId.value + '/revise', {
      feedback: reviewComment.value,
    })
    if (updated) issue.value = updated
    else await load()
    reviewComment.value = ''
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    alert('Failed to request changes: ' + msg)
  } finally {
    requestingChanges.value = false
  }
}

async function submitRevision() {
  if (!reviseFeedback.value.trim()) return
  reviseSubmitting.value = true
  try {
    const updated = await apiPost<Issue>('/api/issues/' + issueId.value + '/revise', {
      feedback: reviseFeedback.value,
    })
    if (updated) issue.value = updated
    else await load()
    reviseFeedback.value = ''
    showReviseForm.value = false
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    alert('Revision failed: ' + msg)
  } finally {
    reviseSubmitting.value = false
  }
}

function pollAll() {
  load()
  loadComments()
  loadChildren()
  loadTree()
}

const { markUpdated, lastUpdatedAgo } = usePolling(isActive, pollAll, 3000)

function handleReload() {
  load()
  loadComments()
}

function resetAndLoad() {
  issue.value = null
  loading.value = true
  error.value = null
  comments.value = []
  childIssues.value = []
  treeDescendants.value = []
  diffData.value = null
  diffLoaded.value = false
  prInfo.value = null
  reviewTab.value = 'diff'
  showReviseForm.value = false
  reviseFeedback.value = ''
  load()
  loadComments()
  loadChildren()
  loadTree()
}

watch(issueId, resetAndLoad)
resetAndLoad()
</script>

<template>
  <div class="px-8 py-7 max-md:px-4 max-w-[1200px]">
    <div class="flex items-center justify-between mb-5">
      <button @click="router.back()"
              class="inline-flex items-center gap-1 text-[13px] text-text-secondary bg-transparent border-none cursor-pointer p-0 hover:text-text-primary">
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7"/>
        </svg>
        Back
      </button>
      <span class="text-[11px] text-text-dim">Updated {{ lastUpdatedAgo() }}</span>
    </div>

    <div v-if="loading" class="text-text-dim text-[13px]">Loading...</div>
    <div v-else-if="error" class="text-error text-[13px]">{{ error }}</div>

    <template v-else-if="issue">
      <!-- Header -->
      <div class="flex items-start justify-between mb-5">
        <div>
          <h1 class="text-xl font-bold text-text-primary tracking-tight leading-tight">{{ issue.title }}</h1>
          <div class="flex items-center gap-3 mt-1.5">
            <span class="font-mono text-xs text-text-secondary">{{ issue.id }}</span>
            <StatusBadge :status="issue.status" />
          </div>
          <div class="text-xs text-text-secondary mt-1">
            Created {{ relativeTime(issue.created_at) }}
            <template v-if="issue.completed_at"> &middot; {{ duration(issue.created_at, issue.completed_at) }}</template>
            <template v-else-if="issue.status === 'running'"> &middot; <span class="text-info">{{ duration(issue.created_at, null) }}</span></template>
          </div>
        </div>
        <div class="flex items-center gap-2">
          <button v-if="canRevise" @click="showReviseForm = !showReviseForm"
                  class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-warning text-black hover:opacity-85">
            Re-run with Feedback
          </button>
          <button v-if="canCancel" @click="cancelIssue" :disabled="cancelling"
                  class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-error text-white hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
            {{ cancelling ? 'Cancelling...' : 'Cancel Issue' }}
          </button>
        </div>
      </div>

      <!-- Review Banner -->
      <ReviewBanner v-if="canReview" :approving="approving"
                    @approve="approveIssue" @request-changes="reviewTab = 'feedback'" />

      <!-- Revision Form -->
      <div v-if="showReviseForm" class="bg-surface border border-warning/30 rounded-lg mb-4">
        <div class="px-4 py-3.5 border-b border-warning/30">
          <span class="text-[13px] font-semibold text-text-primary">Re-run with Feedback</span>
        </div>
        <div class="p-4">
          <textarea v-model="reviseFeedback" rows="4"
                    class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none resize-y min-h-16 leading-relaxed transition-colors focus:border-border-focus"
                    placeholder="Describe what should change..."></textarea>
          <div class="flex gap-2 mt-3">
            <button @click="submitRevision" :disabled="reviseSubmitting || !reviseFeedback.trim()"
                    class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-warning text-black hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
              {{ reviseSubmitting ? 'Submitting...' : 'Submit & Re-run' }}
            </button>
            <button @click="showReviseForm = false; reviseFeedback = ''"
                    class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border border-border cursor-pointer bg-transparent text-text-secondary hover:text-text-primary hover:border-border-focus">
              Cancel
            </button>
          </div>
        </div>
      </div>

      <!-- Parent link -->
      <div v-if="issue.parent_issue_id"
           class="flex items-center gap-1.5 text-xs text-text-secondary mb-4 px-3 py-2 bg-surface border border-border rounded-md">
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7"/>
        </svg>
        Child of
        <router-link :to="'/issues/' + issue.parent_issue_id"
                     class="text-accent font-mono text-xs no-underline hover:text-accent-hover hover:underline">
          {{ issue.parent_issue_id }}
        </router-link>
      </div>

      <!-- Worktree info -->
      <div v-if="hasWorktree" class="flex items-center gap-3 text-xs mb-4 px-3 py-2 bg-surface border border-border rounded-md">
        <svg class="w-3.5 h-3.5 text-text-dim shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"/>
        </svg>
        <span v-if="prInfo?.branch || issue!.context.branch" class="text-text-secondary font-mono">{{ prInfo?.branch || issue!.context.branch }}</span>
        <span class="text-text-dim font-mono truncate" :title="String(issue!.context.work_dir)">{{ issue!.context.work_dir }}</span>
        <div class="ml-auto flex items-center gap-3">
          <a v-if="prInfo?.compare_url" :href="prInfo.compare_url" target="_blank" rel="noopener"
             class="inline-flex items-center gap-1 text-accent hover:text-accent-hover text-xs font-medium whitespace-nowrap no-underline">
            <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 16 16">
              <path d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z"/>
            </svg>
            PR
          </a>
          <button v-if="showTabs" @click="reviewTab = 'files'; if (!diffData) loadDiff()"
                  class="text-accent hover:text-accent-hover bg-transparent border-none cursor-pointer text-xs font-medium whitespace-nowrap">
            Browse files &rarr;
          </button>
        </div>
      </div>

      <!-- Review Tabs -->
      <div v-if="showTabs" class="flex gap-0 border-b border-border mb-4">
        <button :class="['px-4 py-2 text-[13px] font-medium cursor-pointer bg-transparent border-b-2 border-t-0 border-l-0 border-r-0',
                         reviewTab === 'diff' ? 'text-accent border-accent' : 'text-text-secondary border-transparent hover:text-text-primary']"
                @click="reviewTab = 'diff'; if (!diffData) loadDiff()">Diff</button>
        <button :class="['px-4 py-2 text-[13px] font-medium cursor-pointer bg-transparent border-b-2 border-t-0 border-l-0 border-r-0',
                         reviewTab === 'files' ? 'text-accent border-accent' : 'text-text-secondary border-transparent hover:text-text-primary']"
                @click="reviewTab = 'files'; if (!diffData) loadDiff()">Files</button>
        <button v-if="canReview"
                :class="['px-4 py-2 text-[13px] font-medium cursor-pointer bg-transparent border-b-2 border-t-0 border-l-0 border-r-0',
                         reviewTab === 'feedback' ? 'text-accent border-accent' : 'text-text-secondary border-transparent hover:text-text-primary']"
                @click="reviewTab = 'feedback'">Review</button>
      </div>

      <!-- Diff Tab -->
      <div v-if="showTabs && reviewTab === 'diff'">
        <div class="flex justify-end mb-2">
          <button @click="diffLoaded = false; loadDiff(); diffLoaded = true"
                  :disabled="diffLoading"
                  class="inline-flex items-center gap-1 text-xs text-text-secondary hover:text-text-primary bg-transparent border-none cursor-pointer disabled:opacity-40">
            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/>
            </svg>
            Refresh
          </button>
        </div>
        <DiffViewer :data="diffData" :loading="diffLoading" />
      </div>

      <!-- Files Tab -->
      <FileViewer v-if="showTabs && reviewTab === 'files'" :issue-id="issueId" :data="diffData" :loading="diffLoading" />

      <!-- Feedback Tab -->
      <div v-if="showTabs && reviewTab === 'feedback' && canReview" class="bg-surface border border-border rounded-lg mb-4">
        <div class="px-4 py-3.5 border-b border-border">
          <span class="text-[13px] font-semibold text-text-primary">Review Feedback</span>
        </div>
        <div class="p-4">
          <textarea v-model="reviewComment" rows="4"
                    class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none resize-y min-h-16 leading-relaxed transition-colors focus:border-border-focus"
                    placeholder="Describe the changes you want..."></textarea>
          <div class="flex gap-2 mt-3">
            <button @click="approveIssue" :disabled="approving"
                    class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-success text-white hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
              {{ approving ? 'Approving...' : 'Approve' }}
            </button>
            <button @click="requestChanges" :disabled="requestingChanges || !reviewComment.trim()"
                    class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-warning text-black hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
              {{ requestingChanges ? 'Submitting...' : 'Request Changes' }}
            </button>
          </div>
        </div>
      </div>

      <MetaGrid :issue="issue" @update:tags="tags => issue.tags = tags" />

      <!-- Body -->
      <div v-if="issue.body" class="text-[13px] text-text-primary whitespace-pre-wrap break-words font-mono leading-relaxed bg-surface border border-border rounded-md px-3 py-2.5 mb-3">{{ issue.body }}</div>

      <!-- Live streaming output -->
      <StreamPanel v-if="streamConnected || streamEvents.length"
                   :events="streamEvents"
                   :connected="streamConnected" />

      <!-- Result (from tagged comment) -->
      <div v-if="resultComment" class="border-l-2 border-success/40 pl-3 mb-3">
        <div class="text-[10px] font-semibold text-success uppercase tracking-wider mb-1">Result</div>
        <pre class="font-mono text-[13px] text-text-primary whitespace-pre-wrap break-words leading-relaxed m-0">{{ resultComment.body }}</pre>
      </div>

      <!-- Error -->
      <div v-if="issue.error" class="border-l-2 border-error/40 pl-3 mb-3">
        <div class="text-[10px] font-semibold text-error uppercase tracking-wider mb-1">Error</div>
        <pre class="font-mono text-[13px] text-text-primary whitespace-pre-wrap break-words leading-relaxed m-0">{{ issue.error }}</pre>
      </div>

      <!-- Issue Tree (replaces flat child list when tree data available) -->
      <DagTree v-if="issue && treeDescendants.length" :root="issue" :descendants="treeDescendants" />

      <!-- Flat child list fallback (only when no tree data) -->
      <div v-else-if="childIssues.length" class="bg-surface border border-border rounded-lg mb-4">
        <div class="px-4 py-3.5 border-b border-border flex items-center justify-between">
          <span class="text-[13px] font-semibold text-text-primary">Child Issues ({{ childIssues.length }})</span>
        </div>
        <div>
          <div v-for="child in childIssues" :key="child.id"
               @click="router.push('/issues/' + child.id)"
               class="flex items-center px-4 py-2.5 border-b border-border last:border-b-0 text-[13px] cursor-pointer transition-colors hover:bg-hover">
            <StatusBadge :status="child.status" />
            <span class="font-mono text-xs text-text-dim ml-2">{{ child.id }}</span>
            <span class="ml-2 text-text-primary">{{ truncate(child.title, 60) }}</span>
          </div>
        </div>
      </div>

      <!-- Context -->
      <div v-if="issue.context && Object.keys(issue.context).length" class="mb-3">
        <button @click="showContext = !showContext"
                class="flex items-center gap-1.5 text-xs text-text-dim bg-transparent border-none cursor-pointer p-0 hover:text-text-secondary">
          <span>{{ showContext ? '\u25BE' : '\u25B8' }}</span>
          <span>Context</span>
        </button>
        <div v-if="showContext" class="mt-2 pl-3 border-l border-border">
          <JsonTree :data="issue.context" :start-expanded="true" />
        </div>
      </div>

      <!-- Dependencies -->
      <div v-if="issue.dependencies?.length" class="flex items-center gap-2 text-xs text-text-dim mb-3 px-1">
        <span>Depends on</span>
        <template v-for="(depId, i) in issue.dependencies" :key="depId">
          <span v-if="i > 0" class="text-text-dim">,</span>
          <router-link :to="'/issues/' + depId" class="font-mono text-accent no-underline hover:text-accent-hover hover:underline">{{ depId }}</router-link>
        </template>
      </div>

      <!-- Comments -->
      <CommentSection :issue-id="issueId" :comments="comments" :can-revise="!!canRevise" @reload="handleReload" />
    </template>
  </div>
</template>
