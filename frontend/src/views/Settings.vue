<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api, apiPut, apiDelete } from '../api'

type SettingsMap = Record<string, string>

const settings = ref<{ key: string; value: string; saving: boolean; deleting: boolean }[]>([])
const loading = ref(true)
const error = ref('')

const newKey = ref('')
const newValue = ref('')
const adding = ref(false)

interface SettingSchema {
  key: string
  description: string
  default: string
}

const schema = ref<SettingSchema[]>([])
const knownSettings = ref<Record<string, SettingSchema>>({})

async function load() {
  try {
    const [data, schemaDefs] = await Promise.all([
      api<SettingsMap>('/api/settings'),
      api<SettingSchema[]>('/api/settings/schema'),
    ])
    schema.value = schemaDefs
    knownSettings.value = Object.fromEntries(schemaDefs.map(s => [s.key, s]))
    settings.value = Object.entries(data).map(([key, value]) => ({
      key,
      value: String(value),
      saving: false,
      deleting: false,
    }))
  } catch (e: unknown) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

async function saveSetting(entry: typeof settings.value[number]) {
  entry.saving = true
  try {
    await apiPut('/api/settings', { [entry.key]: entry.value })
  } catch (e: unknown) {
    alert(`Failed to save: ${e instanceof Error ? e.message : String(e)}`)
  } finally {
    entry.saving = false
  }
}

async function deleteSetting(entry: typeof settings.value[number]) {
  entry.deleting = true
  try {
    await apiDelete(`/api/settings/${encodeURIComponent(entry.key)}`)
    settings.value = settings.value.filter(s => s.key !== entry.key)
  } catch (e: unknown) {
    alert(`Failed to delete: ${e instanceof Error ? e.message : String(e)}`)
  } finally {
    entry.deleting = false
  }
}

async function addSetting() {
  const key = newKey.value.trim()
  const value = newValue.value.trim()
  if (!key) return
  adding.value = true
  try {
    await apiPut('/api/settings', { [key]: value })
    settings.value.push({ key, value, saving: false, deleting: false })
    newKey.value = ''
    newValue.value = ''
  } catch (e: unknown) {
    alert(`Failed to add: ${e instanceof Error ? e.message : String(e)}`)
  } finally {
    adding.value = false
  }
}

function placeholder(key: string): string {
  const s = knownSettings.value[key]
  return s ? `default: ${s.default}` : ''
}

onMounted(load)
</script>

<template>
  <div class="px-8 py-7 max-md:px-4 max-w-[1400px]">
    <div class="flex items-center justify-between mb-6">
      <h2 class="text-xl font-bold text-text-primary tracking-tight">Settings</h2>
    </div>

    <div v-if="loading" class="text-text-dim text-[13px] text-center py-8">Loading...</div>
    <div v-else-if="error" class="text-error text-[13px] text-center py-8">{{ error }}</div>

    <template v-else>
      <div class="bg-surface border border-border rounded-lg mb-6">
        <div class="overflow-x-auto">
          <table class="w-full border-collapse text-[13px]">
            <thead>
              <tr>
                <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Key</th>
                <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap">Value</th>
                <th class="px-4 py-2.5 text-left text-[11px] font-semibold text-text-dim uppercase tracking-wider border-b border-border whitespace-nowrap w-[160px]">Actions</th>
              </tr>
            </thead>
            <tbody>
              <tr v-if="settings.length === 0">
                <td colspan="3" class="px-4 py-8 text-center text-text-dim">No settings configured</td>
              </tr>
              <tr v-for="entry in settings" :key="entry.key" class="transition-colors hover:bg-hover">
                <td class="px-4 py-2.5 border-b border-border">
                  <div class="font-mono text-xs text-text-primary">{{ entry.key }}</div>
                  <div v-if="knownSettings[entry.key]" class="text-[11px] text-text-dim mt-0.5">{{ knownSettings[entry.key].description }}</div>
                </td>
                <td class="px-4 py-2.5 border-b border-border">
                  <input v-model="entry.value"
                         :placeholder="placeholder(entry.key)"
                         class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus" />
                </td>
                <td class="px-4 py-2.5 border-b border-border">
                  <div class="flex gap-2">
                    <button @click="saveSetting(entry)"
                            :disabled="entry.saving"
                            class="inline-flex items-center justify-center gap-1 px-2.5 py-1 text-[11px] font-semibold rounded-md border-none cursor-pointer transition-colors bg-accent text-white hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed">
                      {{ entry.saving ? 'Saving...' : 'Save' }}
                    </button>
                    <button @click="deleteSetting(entry)"
                            :disabled="entry.deleting"
                            class="inline-flex items-center justify-center gap-1 px-2.5 py-1 text-[11px] font-semibold rounded-md border-none cursor-pointer transition-colors bg-error text-white hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
                      {{ entry.deleting ? 'Deleting...' : 'Delete' }}
                    </button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="bg-surface border border-border rounded-lg">
        <div class="px-4 py-3.5 border-b border-border">
          <span class="text-[13px] font-semibold text-text-primary">Add Setting</span>
        </div>
        <div class="p-4">
          <div class="flex gap-3 items-end flex-wrap">
            <div class="flex-1 min-w-[200px]">
              <label class="block text-[11px] font-semibold text-text-dim uppercase tracking-wider mb-1.5">Key</label>
              <input v-model="newKey"
                     placeholder="e.g. executor.timeout_secs"
                     class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus" />
            </div>
            <div class="flex-1 min-w-[200px]">
              <label class="block text-[11px] font-semibold text-text-dim uppercase tracking-wider mb-1.5">Value</label>
              <input v-model="newValue"
                     placeholder="e.g. 7200"
                     class="w-full py-1.5 px-2.5 text-[13px] font-mono text-text-primary bg-input border border-border rounded-md outline-none transition-colors focus:border-border-focus" />
            </div>
            <button @click="addSetting"
                    :disabled="adding || !newKey.trim()"
                    class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer transition-colors bg-accent text-white hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed">
              {{ adding ? 'Adding...' : 'Add' }}
            </button>
          </div>
        </div>
      </div>

      <div class="mt-6 bg-surface border border-border rounded-lg p-4">
        <div class="text-[11px] font-semibold text-text-dim uppercase tracking-wider mb-3">Known Settings</div>
        <div class="grid grid-cols-1 gap-1.5">
          <div v-for="s in schema" :key="s.key" class="text-[12px]">
            <span class="font-mono text-text-secondary">{{ s.key }}</span>
            <span class="text-text-dim"> — {{ s.description }}</span>
            <span v-if="s.default" class="text-text-dim"> (default: {{ s.default }})</span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
