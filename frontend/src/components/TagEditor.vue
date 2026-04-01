<script setup lang="ts">
import { ref, nextTick } from 'vue'

const props = defineProps<{
  tags: string[]
  issueId: string
}>()

const emit = defineEmits<{
  update: [tags: string[]]
}>()

const editing = ref(false)
const newTag = ref('')
const inputEl = ref<HTMLInputElement>()

function removeTag(index: number) {
  const updated = [...props.tags]
  updated.splice(index, 1)
  save(updated)
}

function startEditing() {
  editing.value = true
  newTag.value = ''
  nextTick(() => inputEl.value?.focus())
}

function addTag() {
  const tag = newTag.value.trim().toLowerCase()
  if (!tag || props.tags.includes(tag)) {
    editing.value = false
    newTag.value = ''
    return
  }
  save([...props.tags, tag])
  editing.value = false
  newTag.value = ''
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    addTag()
  } else if (e.key === 'Escape') {
    editing.value = false
    newTag.value = ''
  }
}

async function save(tags: string[]) {
  const resp = await fetch(`/api/issues/${props.issueId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tags }),
  })
  if (resp.ok) emit('update', tags)
}
</script>

<template>
  <div class="flex flex-wrap items-center gap-1.5" @dblclick.stop="startEditing">
    <span
      v-for="(tag, i) in tags"
      :key="tag"
      class="tag-chip"
    >
      {{ tag }}
      <button
        class="tag-remove"
        @click.stop="removeTag(i)"
        title="Remove tag"
      >&times;</button>
    </span>
    <span v-if="!tags.length && !editing" class="text-text-dim font-mono text-xs">no tags</span>
    <input
      v-if="editing"
      ref="inputEl"
      v-model="newTag"
      class="tag-input"
      placeholder="tag"
      @keydown="onKeydown"
      @blur="addTag"
    />
    <button
      v-if="!editing"
      class="tag-add"
      @click.stop="startEditing"
      title="Add tag"
    >+</button>
  </div>
</template>

<style scoped>
.tag-chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 1px 8px;
  border-radius: 9999px;
  background: var(--color-elevated);
  border: 1px solid var(--color-border);
  color: var(--color-text-secondary);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 18px;
  white-space: nowrap;
}

.tag-remove {
  margin-left: 2px;
  color: var(--color-text-dim);
  font-size: 13px;
  line-height: 1;
  cursor: pointer;
  border-radius: 50%;
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.tag-remove:hover {
  color: var(--color-error);
  background: color-mix(in srgb, var(--color-error) 15%, transparent);
}

.tag-add {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 9999px;
  border: 1px dashed var(--color-border);
  color: var(--color-text-dim);
  font-size: 12px;
  cursor: pointer;
}
.tag-add:hover {
  border-color: var(--color-border-hover);
  color: var(--color-text-secondary);
}

.tag-input {
  padding: 1px 8px;
  border-radius: 9999px;
  background: var(--color-input);
  border: 1px solid var(--color-border-focus);
  color: var(--color-text-primary);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 18px;
  width: 80px;
  outline: none;
}
</style>
