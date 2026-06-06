<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get, upload, del } from '../api';
import type { ScratchFile } from '../types';

// Scratch attachments for a session, as a slim strip that sits in the working
// zone right under the terminal — drop a file here and reference it from the
// agent as `scratch/<name>`. Deliberately one quiet row (not a tall card): the
// terminal is the surface, this is just the side door for handing the agent a
// file. Files show as removable chips inline.
const props = defineProps<{ id: string }>();

const files = ref<ScratchFile[]>([]);
const dragging = ref(false);
const busy = ref(false);
const error = ref('');

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

async function refresh() {
  try {
    files.value = (await get(`/sessions/${props.id}/scratch`)) as ScratchFile[];
  } catch (e) {
    error.value = (e as Error).message;
  }
}

async function uploadFiles(list: FileList | File[]) {
  busy.value = true;
  error.value = '';
  try {
    for (const file of Array.from(list)) {
      await upload(`/sessions/${props.id}/scratch?name=${encodeURIComponent(file.name)}`, file);
    }
    await refresh();
  } catch (e) {
    error.value = (e as Error).message;
  } finally {
    busy.value = false;
  }
}

function onDrop(e: DragEvent) {
  dragging.value = false;
  const dropped = e.dataTransfer?.files;
  if (dropped && dropped.length) uploadFiles(dropped);
}

const fileInput = ref<HTMLInputElement | null>(null);
function onPick(e: Event) {
  const input = e.target as HTMLInputElement;
  if (input.files && input.files.length) uploadFiles(input.files);
  input.value = '';
}

async function remove(name: string) {
  try {
    await del(`/sessions/${props.id}/scratch?name=${encodeURIComponent(name)}`);
    await refresh();
  } catch (e) {
    error.value = (e as Error).message;
  }
}

onMounted(refresh);
</script>

<template>
  <section data-testid="scratch-panel">
    <div
      class="flex flex-wrap items-center gap-x-2 gap-y-1.5 rounded border border-dashed px-3 py-2 text-xs transition-colors cursor-pointer"
      :class="dragging ? 'border-accent bg-accent/10' : 'border-line hover:border-accent/60'"
      data-testid="scratch-dropzone"
      @dragover.prevent="dragging = true"
      @dragleave.prevent="dragging = false"
      @drop.prevent="onDrop"
      @click="fileInput?.click()"
    >
      <span class="text-faint">📎</span>
      <span :class="dragging ? 'text-fg' : 'text-muted'">
        {{ busy ? 'Uploading…' : 'Drop a file or click to attach' }}
      </span>
      <span class="text-faint">·</span>
      <span class="font-mono text-faint">reference as <code>scratch/&lt;name&gt;</code></span>

      <ul v-if="files.length" class="ml-auto flex flex-wrap items-center gap-1.5" @click.stop>
        <li v-for="f in files" :key="f.name" class="meta-chip text-fg">
          <span class="truncate">{{ f.name }}</span>
          <span class="text-faint">{{ fmtBytes(f.bytes) }}</span>
          <button
            type="button"
            class="text-faint hover:text-block"
            title="Remove"
            @click.stop="remove(f.name)"
          >
            ✕
          </button>
        </li>
      </ul>

      <input ref="fileInput" type="file" multiple class="hidden" @change="onPick" />
    </div>

    <p v-if="error" class="mt-1 text-xs text-block">{{ error }}</p>
  </section>
</template>
