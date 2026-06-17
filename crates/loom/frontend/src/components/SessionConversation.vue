<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue';
import { get } from '../api';
import type { IrisLog, IrisMessage } from '../types';
import MarkdownView from './MarkdownView.vue';

// The Conversation tab: the agent's chat with the model, rendered for review.
// Reads the normalized iris log from `GET /sessions/{id}/conversation` (live
// transcript when present, else the capture archived on teardown) and renders it
// natively — user/assistant turns, collapsible thinking, tool calls + results —
// so an archived session (whose terminal is gone) is still reviewable here.
const props = defineProps<{ id: string }>();

type LoadState = 'loading' | 'ready' | 'empty' | 'error';
const log = ref<IrisLog | null>(null);
const state = ref<LoadState>('loading');
const errorMsg = ref('');

async function load() {
  state.value = 'loading';
  try {
    const data = (await get(`/sessions/${props.id}/conversation`)) as IrisLog;
    log.value = data;
    state.value = data && data.messages.length ? 'ready' : 'empty';
  } catch (e) {
    // A 404 means nothing's been recorded (a shell session, or not yet) — that's
    // an empty state, not an error worth shouting about.
    const msg = (e as Error).message ?? '';
    if (/not found|conversation/i.test(msg)) {
      state.value = 'empty';
    } else {
      errorMsg.value = msg;
      state.value = 'error';
    }
  }
}

onMounted(load);
watch(() => props.id, load);

// One banner line: source agent · model · message count · time span.
const banner = computed(() => {
  const l = log.value;
  if (!l) return '';
  const parts: string[] = [];
  if (l.source) parts.push(l.source);
  parts.push(`${l.messages.length} messages`);
  if (l.model) parts.push(l.model);
  const times = l.messages.map((m) => m.timestamp).filter(Boolean) as string[];
  if (times.length) parts.push(`${shortTime(times[0])} – ${shortTime(times[times.length - 1])}`);
  return parts.join(' · ');
});

function shortTime(ts?: string): string {
  if (!ts) return '';
  return ts.length >= 19 && ts[10] === 'T' ? ts.slice(11, 19) : ts;
}

// An assistant heading opens each contiguous assistant run; a tool call/result
// emitted as its own message must not start a fresh heading.
function startsAssistantRun(messages: IrisMessage[], i: number): boolean {
  return i === 0 || messages[i - 1].role !== 'assistant';
}

// A shell command (Bash `command`, Codex `cmd`) renders as a shell block; any
// other tool input as pretty JSON or its raw string.
function toolCommand(input: unknown): string | null {
  if (input && typeof input === 'object') {
    const o = input as Record<string, unknown>;
    const c = o.command ?? o.cmd;
    if (typeof c === 'string') return c;
  }
  return null;
}
function toolBody(input: unknown): string {
  if (typeof input === 'string') return input;
  try {
    return JSON.stringify(input, null, 2);
  } catch {
    return String(input);
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Banner + refresh: a live session's conversation grows, so allow a manual
         reload without re-opening the tab. -->
    <div class="mb-2 flex items-center justify-between gap-2">
      <p class="truncate text-xs text-muted">{{ banner }}</p>
      <button
        type="button"
        class="btn-secondary shrink-0 text-xs"
        :disabled="state === 'loading'"
        @click="load"
      >
        Refresh
      </button>
    </div>

    <p v-if="state === 'loading'" class="text-sm text-muted">Loading conversation…</p>
    <p v-else-if="state === 'error'" class="text-sm text-block">{{ errorMsg }}</p>
    <p v-else-if="state === 'empty'" class="text-sm text-muted">
      No conversation recorded for this session yet.
    </p>

    <div v-else class="min-h-0 flex-1 space-y-4 overflow-auto pb-2">
      <template v-for="(msg, i) in log!.messages" :key="i">
        <!-- Injected context (primers, system/permissions) — tucked away. -->
        <details v-if="msg.role === 'context'" class="rounded border border-line bg-subtle/40 px-3 py-2">
          <summary class="cursor-pointer text-xs text-muted">📎 Context</summary>
          <div class="mt-2 space-y-2">
            <template v-for="(b, j) in msg.blocks" :key="j">
              <pre
                v-if="b.kind === 'text' || b.kind === 'thinking'"
                class="whitespace-pre-wrap break-words font-mono text-xs text-muted"
                >{{ b.text }}</pre>
            </template>
          </div>
        </details>

        <!-- User turn. -->
        <section v-else-if="msg.role === 'user'" class="rounded border-l-2 border-accent bg-subtle/40 px-3 py-2">
          <header class="mb-1 text-xs font-medium text-accent">
            🧑 User<span v-if="msg.timestamp" class="ml-2 font-normal text-muted">{{ shortTime(msg.timestamp) }}</span>
          </header>
          <template v-for="(b, j) in msg.blocks" :key="j">
            <MarkdownView v-if="b.kind === 'text'" :id="props.id" path="" :source="b.text" />
            <p v-else-if="b.kind === 'image'" class="text-xs italic text-muted">[image]</p>
          </template>
        </section>

        <!-- Assistant turn (one heading per run). -->
        <section v-else>
          <header v-if="startsAssistantRun(log!.messages, i)" class="mb-1 text-xs font-medium text-fg">
            🤖 Assistant<span v-if="msg.timestamp" class="ml-2 font-normal text-muted">{{ shortTime(msg.timestamp) }}</span>
          </header>
          <template v-for="(b, j) in msg.blocks" :key="j">
            <MarkdownView v-if="b.kind === 'text'" :id="props.id" path="" :source="b.text" class="mb-2" />

            <details v-else-if="b.kind === 'thinking'" class="mb-2 rounded border border-line bg-subtle/40 px-3 py-1.5">
              <summary class="cursor-pointer text-xs text-muted">💭 Thinking</summary>
              <pre class="mt-2 whitespace-pre-wrap break-words font-mono text-xs text-muted">{{ b.text }}</pre>
            </details>

            <div v-else-if="b.kind === 'tool_use'" class="mb-2 overflow-hidden rounded border border-line">
              <div class="border-b border-line bg-subtle/60 px-3 py-1 text-xs font-medium text-fg">
                🔧 {{ b.name }}
              </div>
              <pre class="overflow-x-auto px-3 py-2 font-mono text-xs text-fg">{{ toolCommand(b.input) ?? toolBody(b.input) }}</pre>
            </div>

            <details v-else-if="b.kind === 'tool_result'" class="mb-2 rounded border border-line bg-subtle/30 px-3 py-1.5">
              <summary class="cursor-pointer text-xs" :class="b.is_error ? 'text-block' : 'text-muted'">
                ↳ result{{ b.is_error ? ' (error)' : '' }}
              </summary>
              <pre class="mt-2 overflow-x-auto whitespace-pre-wrap break-words font-mono text-xs text-muted">{{ b.output }}</pre>
            </details>

            <p v-else-if="b.kind === 'image'" class="mb-2 text-xs italic text-muted">[image]</p>
          </template>
        </section>
      </template>
    </div>
  </div>
</template>
