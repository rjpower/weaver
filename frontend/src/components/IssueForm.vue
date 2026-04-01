<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

const emit = defineEmits<{
  close: []
}>()

const router = useRouter()
const form = ref({ title: '', body: '', tags: '', priority: '0', branch: '' })
const error = ref<string | null>(null)
const submitting = ref(false)

async function submit() {
  error.value = null
  submitting.value = true
  try {
    const context: Record<string, string> = {}
    if (form.value.branch.trim()) {
      context.branch = form.value.branch.trim()
    }
    const body = {
      title: form.value.title,
      body: form.value.body,
      tags: form.value.tags.split(',').map(s => s.trim()).filter(Boolean),
      priority: parseInt(form.value.priority) || 0,
      ...(Object.keys(context).length > 0 && { context }),
    }
    const resp = await fetch('/api/issues', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!resp.ok) {
      const text = await resp.text()
      error.value = text || `Error ${resp.status}`
      return
    }
    const data = await resp.json()
    emit('close')
    router.push('/issues/' + data.id)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="bg-surface border border-border rounded-lg p-5 mb-4">
    <h3 class="text-sm font-bold text-text-primary uppercase tracking-wider mb-3">New Issue</h3>
    <div class="mb-3">
      <label class="block text-xs text-text-secondary mb-1">Title</label>
      <input v-model="form.title"
             class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus"
             placeholder="Issue title">
    </div>
    <div class="mb-3">
      <label class="block text-xs text-text-secondary mb-1">Body</label>
      <textarea v-model="form.body" rows="8"
                class="w-full py-1.5 px-2.5 text-xs font-mono text-text-primary bg-input border border-border rounded-md outline-none resize-y min-h-16 leading-relaxed transition-colors focus:border-border-focus"
                placeholder="Describe what needs to be done..."></textarea>
    </div>
    <div class="mb-3">
      <label class="block text-xs text-text-secondary mb-1">Branch name</label>
      <input v-model="form.branch"
             class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus"
             placeholder="feature/my-branch (optional)">
    </div>
    <div class="grid grid-cols-2 gap-3 mb-3">
      <div>
        <label class="block text-xs text-text-secondary mb-1">Tags (comma-separated)</label>
        <input v-model="form.tags"
               class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus"
               placeholder="bug, urgent">
      </div>
      <div>
        <label class="block text-xs text-text-secondary mb-1">Priority</label>
        <input v-model="form.priority" type="number"
               class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus"
               placeholder="0">
      </div>
    </div>
    <div v-if="error" class="text-error text-[13px] mb-3">{{ error }}</div>
    <div class="flex gap-2">
      <button @click="submit" :disabled="submitting"
              class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer transition-colors bg-accent text-white hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed">
        {{ submitting ? 'Creating...' : 'Create' }}
      </button>
      <button @click="emit('close')"
              class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer transition-colors bg-elevated text-text-secondary hover:text-text-primary hover:bg-hover">
        Cancel
      </button>
    </div>
  </div>
</template>
