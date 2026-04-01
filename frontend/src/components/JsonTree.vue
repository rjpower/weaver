<script setup lang="ts">
import { ref } from 'vue'

const props = withDefaults(defineProps<{
  data: unknown
  label?: string
  startExpanded?: boolean
}>(), {
  label: '',
  startExpanded: false,
})

const expanded = ref(props.startExpanded)

function isObject(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function entries(data: unknown): [string, unknown][] {
  if (Array.isArray(data)) return data.map((v, i) => [String(i), v])
  if (isObject(data)) return Object.entries(data)
  return []
}

function previewText(data: unknown): string {
  if (Array.isArray(data)) return `[${data.length} items]`
  return '{...}'
}

function brackets(data: unknown): [string, string] {
  return Array.isArray(data) ? ['[', ']'] : ['{', '}']
}
</script>

<template>
  <!-- Primitives -->
  <div v-if="data === null || data === undefined" class="font-mono text-xs leading-relaxed whitespace-nowrap">
    <span v-if="label" class="text-info">{{ label }}: </span>
    <span class="text-text-dim italic">null</span>
  </div>

  <div v-else-if="typeof data === 'boolean'" class="font-mono text-xs leading-relaxed whitespace-nowrap">
    <span v-if="label" class="text-info">{{ label }}: </span>
    <span class="text-warning">{{ data }}</span>
  </div>

  <div v-else-if="typeof data === 'number'" class="font-mono text-xs leading-relaxed whitespace-nowrap">
    <span v-if="label" class="text-info">{{ label }}: </span>
    <span class="text-accent">{{ data }}</span>
  </div>

  <div v-else-if="typeof data === 'string'" class="font-mono text-xs leading-relaxed whitespace-nowrap">
    <span v-if="label" class="text-info">{{ label }}: </span>
    <span class="text-success break-all whitespace-pre-wrap">"{{ data.length > 200 ? data.substring(0, 200) + '...' : data }}"</span>
  </div>

  <!-- Objects/Arrays -->
  <div v-else>
    <!-- Empty container -->
    <div v-if="entries(data).length === 0" class="font-mono text-xs leading-relaxed whitespace-nowrap">
      <span v-if="label" class="text-info">{{ label }}: </span>
      {{ brackets(data)[0] }}{{ brackets(data)[1] }}
    </div>

    <!-- Non-empty container -->
    <div v-else>
      <div class="font-mono text-xs leading-relaxed whitespace-nowrap">
        <span class="text-text-dim cursor-pointer select-none hover:text-text-primary"
              @click="expanded = !expanded">{{ expanded ? '\u25BE ' : '\u25B8 ' }}</span>
        <span v-if="label" class="text-info">{{ label }}: </span>
        <template v-if="!expanded">
          <span class="text-text-dim">{{ previewText(data) }}</span>
        </template>
        <template v-else>{{ brackets(data)[0] }}</template>
      </div>
      <div v-if="expanded" class="pl-4 border-l border-border ml-1">
        <JsonTree v-for="[key, val] in entries(data)" :key="key"
                  :data="val" :label="key" :start-expanded="false" />
      </div>
      <div v-if="expanded" class="font-mono text-xs leading-relaxed">
        {{ brackets(data)[1] }}
      </div>
    </div>
  </div>
</template>

<script lang="ts">
export default { name: 'JsonTree' }
</script>
