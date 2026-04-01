<script setup lang="ts">
import { ref } from 'vue'
import { api } from '../api'
import type { DiffResponse, FileContentResponse } from '../types'

const props = defineProps<{
  issueId: string
  data: DiffResponse | null
  loading: boolean
}>()

const fileContent = ref<FileContentResponse | null>(null)
const fileLoading = ref(false)

async function loadFile(path: string) {
  fileLoading.value = true
  try {
    fileContent.value = await api<FileContentResponse>(`/api/issues/${props.issueId}/files/${encodeURIComponent(path)}`)
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    fileContent.value = { path, content: 'Error loading file: ' + msg }
  } finally {
    fileLoading.value = false
  }
}
</script>

<template>
  <div class="bg-surface border border-border rounded-lg mb-4">
    <div class="px-4 py-3.5 border-b border-border flex items-center justify-between">
      <span class="text-[13px] font-semibold text-text-primary">Changed Files</span>
      <span v-if="data" class="font-mono text-xs text-text-dim">{{ (data.files_changed || []).length }} files</span>
    </div>
    <div v-if="loading" class="p-4 text-text-dim text-[13px]">Loading...</div>
    <div v-else-if="fileContent">
      <div class="px-4 py-2 border-b border-border flex justify-between items-center">
        <span class="font-mono text-xs">{{ fileContent.path }}</span>
        <button @click="fileContent = null"
                class="px-2.5 py-1 text-[11px] font-semibold rounded-md border-none cursor-pointer bg-elevated text-text-secondary hover:text-text-primary hover:bg-hover">
          Back
        </button>
      </div>
      <pre class="p-4 m-0 text-xs overflow-x-auto whitespace-pre text-text-primary">{{ fileContent.content }}</pre>
    </div>
    <div v-else-if="data?.files_changed?.length">
      <div v-for="f in data.files_changed" :key="f"
           @click="loadFile(f)"
           class="flex items-center gap-2 px-4 py-2 border-b border-border last:border-b-0 cursor-pointer text-[13px] font-mono text-text-primary transition-colors hover:bg-hover">
        {{ f }}
      </div>
    </div>
    <div v-else class="p-4 text-text-dim text-[13px]">No changed files</div>
  </div>
</template>
