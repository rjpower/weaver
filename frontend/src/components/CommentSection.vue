<script setup lang="ts">
import { ref, reactive } from 'vue'
import { apiPost } from '../api'
import { relativeTime } from '../utils'
import type { Comment } from '../types'

const props = defineProps<{
  issueId: string
  comments: Comment[]
  canRevise: boolean
}>()

const emit = defineEmits<{
  reload: []
}>()

const newBody = ref('')
const reviseTags = ref('')
const submitting = ref(false)
const expandedIds = reactive(new Set<number>())

async function submit() {
  if (!newBody.value.trim()) return
  submitting.value = true
  try {
    if (props.canRevise) {
      const payload: Record<string, unknown> = { feedback: newBody.value }
      const tagsStr = reviseTags.value.trim()
      if (tagsStr) {
        payload.tags = tagsStr.split(',').map(t => t.trim()).filter(Boolean)
      }
      await apiPost(`/api/issues/${props.issueId}/revise`, payload)
      newBody.value = ''
      reviseTags.value = ''
    } else {
      await apiPost(`/api/issues/${props.issueId}/comments`, {
        author: 'user',
        body: newBody.value,
      })
      newBody.value = ''
    }
    emit('reload')
  } catch (e: unknown) {
    const action = props.canRevise ? 'Revision' : 'Comment'
    const msg = e instanceof Error ? e.message : String(e)
    alert(`${action} failed: ${msg}`)
  } finally {
    submitting.value = false
  }
}

function toggleExpanded(id: number) {
  if (expandedIds.has(id)) expandedIds.delete(id)
  else expandedIds.add(id)
}

const nonPromptComments = (comments: Comment[]) =>
  comments.filter(c => c.tag !== 'generated' && c.tag !== 'result')

const promptComments = (comments: Comment[]) =>
  comments.filter(c => c.tag === 'generated')
</script>

<template>
  <div class="mb-4">
    <!-- Activity feed (non-prompt, non-result comments only) -->
    <div v-if="nonPromptComments(comments).length" class="mb-3">
      <div class="text-xs font-medium text-text-dim mb-2 px-1">Activity</div>
      <div class="space-y-1.5">
        <div v-for="c in nonPromptComments(comments)" :key="c.id"
             class="flex items-start gap-2 text-[13px] px-1">
          <span class="text-text-dim text-[11px] shrink-0 mt-0.5 min-w-[52px] text-right">{{ relativeTime(c.created_at) }}</span>
          <span class="font-semibold text-text-secondary text-xs shrink-0 mt-px">{{ c.author }}</span>
          <span class="text-text-primary whitespace-pre-wrap break-words">{{ c.body }}</span>
        </div>
      </div>
    </div>

    <!-- Prompt log (collapsed, minimal) -->
    <div v-if="promptComments(comments).length" class="mb-3">
      <div v-for="c in promptComments(comments)" :key="c.id">
        <button @click="toggleExpanded(c.id)"
                class="flex items-center gap-1.5 text-[11px] text-text-dim bg-transparent border-none cursor-pointer p-0 px-1 hover:text-text-secondary">
          <span>{{ expandedIds.has(c.id) ? '\u25BE' : '\u25B8' }}</span>
          <span>prompt</span>
          <span class="ml-1">{{ relativeTime(c.created_at) }}</span>
        </button>
        <pre v-if="expandedIds.has(c.id)" class="whitespace-pre-wrap text-xs text-text-dim max-h-[400px] overflow-y-auto m-0 mt-1 ml-4 pl-2 border-l border-border">{{ c.body }}</pre>
      </div>
    </div>

    <!-- Compose -->
    <div class="text-xs font-medium text-text-dim mb-2 px-1">Comments</div>
    <div class="bg-surface border border-border rounded-lg p-3">
      <textarea v-model="newBody" rows="2"
                class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none resize-y min-h-12 leading-relaxed transition-colors focus:border-border-focus"
                :placeholder="canRevise ? 'Revision feedback...' : 'Add a comment...'"></textarea>
      <div v-if="canRevise" class="mt-2">
        <input v-model="reviseTags"
               class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus"
               placeholder="Tags (comma-separated, leave empty to keep current)">
      </div>
      <div class="flex justify-end mt-2">
        <button @click="submit"
                :disabled="submitting || !newBody.trim()"
                :class="[
                  'inline-flex items-center justify-center gap-1.5 px-2.5 py-1 text-[11px] font-semibold rounded-md border-none cursor-pointer transition-colors disabled:opacity-40 disabled:cursor-not-allowed',
                  canRevise
                    ? 'bg-warning text-black hover:opacity-85'
                    : 'bg-accent text-white hover:bg-accent-hover'
                ]">
          {{ submitting ? (canRevise ? 'Requesting...' : 'Posting...') : (canRevise ? 'Request Revision' : 'Post Comment') }}
        </button>
      </div>
    </div>
  </div>
</template>
