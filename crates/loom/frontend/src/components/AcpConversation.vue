<script setup lang="ts">
import {
  ref,
  reactive,
  computed,
  onMounted,
  onActivated,
  onDeactivated,
  onUnmounted,
  watch,
  nextTick,
} from 'vue';
import { getSessionChat, promptSession, interruptSession, answerPermission } from '../api';
import type {
  Session,
  ChatBlock,
  SseDelta,
  SseTool,
  SseTurn,
  UserMessagePayload,
  AgentMessagePayload,
  ThoughtPayload,
  ToolCallPayload,
  PermissionPayload,
  UsagePayload,
  TurnEndPayload,
} from '../types';
import { canSend } from '../lib/sessionState';
import MarkdownView from './MarkdownView.vue';

// The Conversation surface for an *ACP* session (`protocol='acp'`). Its data
// source is the durable chat journal, not the iris scrape: it paints the
// snapshot from `GET /sessions/{id}/chat`, then applies the `/chat/stream` SSE
// tail in place — `block` upserts by (turn, seq), `delta` streams into a shadow
// message/thought, `tool` tracks live tool state, `turn` drives the working
// indicator. The composer posts to `/prompt` (a 202 with `queued:true` when a
// turn is in flight); Stop posts `/interrupt` (`session/cancel`).
const props = defineProps<{ session: Session }>();
const id = computed(() => props.session.id);

// ── The render state the stream feeds ────────────────────────────────────────
// Journaled blocks keyed by `${turn}:${seq}` (idempotent upsert); a shadow
// message/thought per `${turn}:${kind}` accumulating deltas until its block
// journals; live (non-terminal) tool calls by upstream id, superseded by their
// `tool_call` block. Reactive Maps so the render model re-derives on mutation.
const blocks = reactive(new Map<string, ChatBlock>());
const shadows = reactive(new Map<string, { turn: number; kind: 'agent_message' | 'thought'; text: string }>());
const liveTools = reactive(new Map<string, SseTool>());
const turnLive = ref(false);
const liveTurnNo = ref<number | null>(null);
// Optimistic user messages shown the instant Send resolves, before the journaled
// `user_message` block arrives (or, when queued, until it dispatches next turn).
const optimistic = ref<{ text: string; queued: boolean }[]>([]);

const blockKey = (turn: number, seq: number) => `${turn}:${seq}`;

type LoadState = 'loading' | 'ready' | 'error';
const state = ref<LoadState>('loading');
const errorMsg = ref('');

let loadSeq = 0;
async function load({ preserve = false }: { preserve?: boolean } = {}) {
  const seq = ++loadSeq;
  if (!preserve) state.value = 'loading';
  const stick = preserve && nearBottom();
  try {
    const snap = await getSessionChat(id.value);
    if (seq !== loadSeq) return;
    // Rebuild the journal from the authoritative snapshot; live-only state
    // (shadows / live tools) is dropped — the stream re-supplies whatever is
    // still in flight.
    blocks.clear();
    for (const b of snap.blocks) blocks.set(blockKey(b.turn, b.seq), b);
    shadows.clear();
    liveTools.clear();
    liveTurnNo.value = snap.live_turn;
    turnLive.value = snap.live_turn != null;
    state.value = 'ready';
  } catch (e) {
    if (seq !== loadSeq) return;
    if (!preserve) {
      errorMsg.value = (e as Error).message ?? 'Failed to load conversation';
      state.value = 'error';
    }
    return;
  }
  await nextTick();
  if (seq !== loadSeq) return;
  if (stick || !preserve) scrollToBottom();
}

// ── Stream application ───────────────────────────────────────────────────────
function onBlock(b: ChatBlock) {
  blocks.set(blockKey(b.turn, b.seq), b);
  if (b.kind === 'tool_call') {
    const tid = (b.payload as unknown as ToolCallPayload).tool_call_id;
    if (tid) liveTools.delete(tid);
  } else if (b.kind === 'agent_message' || b.kind === 'thought') {
    shadows.delete(`${b.turn}:${b.kind}`);
  } else if (b.kind === 'user_message') {
    // The real prompt landed — drop a matching optimistic echo.
    const text = (b.payload as unknown as UserMessagePayload).text ?? '';
    const i = optimistic.value.findIndex((o) => o.text === text);
    if (i >= 0) optimistic.value.splice(i, 1);
  }
  autoFollow();
}

function onDelta(d: SseDelta) {
  turnLive.value = true;
  liveTurnNo.value = d.turn;
  const k = `${d.turn}:${d.kind}`;
  const cur = shadows.get(k) ?? { turn: d.turn, kind: d.kind, text: '' };
  cur.text += d.text;
  shadows.set(k, cur);
  autoFollow();
}

function onTool(t: SseTool) {
  liveTools.set(t.tool_call_id, t);
  turnLive.value = true;
  liveTurnNo.value = t.turn;
  autoFollow();
}

function onTurn(ev: SseTurn) {
  if (ev.state === 'started') {
    turnLive.value = true;
    liveTurnNo.value = ev.turn;
  } else {
    turnLive.value = false;
    liveTurnNo.value = null;
    // A finished turn resolves any non-queued optimistic echoes.
    optimistic.value = optimistic.value.filter((o) => o.queued);
  }
}

// ── SSE lifecycle (kept-alive discipline) ────────────────────────────────────
let source: EventSource | null = null;
function openStream() {
  source = new EventSource(`/api/sessions/${id.value}/chat/stream`);
  source.addEventListener('block', (e) => onBlock(JSON.parse((e as MessageEvent).data) as ChatBlock));
  source.addEventListener('delta', (e) => onDelta(JSON.parse((e as MessageEvent).data) as SseDelta));
  source.addEventListener('tool', (e) => onTool(JSON.parse((e as MessageEvent).data) as SseTool));
  source.addEventListener('turn', (e) => onTurn(JSON.parse((e as MessageEvent).data) as SseTurn));
}
function closeStream() {
  source?.close();
  source = null;
}

// onMounted owns the first open (onActivated does NOT fire on a lazy v-if mount
// inside an already-active keep-alive); onActivated reopens + catches up on a
// return, guarded by `source` so the initial mount never double-opens.
onMounted(() => {
  openStream();
  load();
});
onActivated(() => {
  if (source) return;
  openStream();
  load({ preserve: true });
});
onDeactivated(closeStream);
onUnmounted(closeStream);
watch(id, () => {
  closeStream();
  openStream();
  load();
});

// ── Composer ─────────────────────────────────────────────────────────────────
const draft = ref('');
const sending = ref(false);
const sendError = ref('');
const composerVisible = computed(() => canSend(props.session));

async function submitPrompt() {
  if (!draft.value.trim() || sending.value) return;
  sending.value = true;
  sendError.value = '';
  const text = draft.value;
  try {
    const ack = await promptSession(id.value, text);
    optimistic.value.push({ text, queued: ack.queued });
    draft.value = '';
    autoFollow();
  } catch (e) {
    sendError.value = (e as Error).message ?? 'Failed to send';
  } finally {
    sending.value = false;
  }
}

const stopping = ref(false);
async function stopTurn() {
  if (stopping.value) return;
  stopping.value = true;
  try {
    await interruptSession(id.value);
  } catch {
    /* the turn may have just ended; the SSE `turn` edge reconciles */
  } finally {
    stopping.value = false;
  }
}

// ── Permission answering ─────────────────────────────────────────────────────
const answering = ref<Set<string>>(new Set());
async function answer(perm: PermissionPayload, optionId: string) {
  if (answering.value.has(perm.request_id)) return;
  answering.value = new Set(answering.value).add(perm.request_id);
  try {
    await answerPermission(id.value, perm.request_id, optionId);
    // The resolved block re-emits over SSE (`block`) and upserts in place.
  } catch {
    /* the request may have been cancelled/resolved already */
  } finally {
    const s = new Set(answering.value);
    s.delete(perm.request_id);
    answering.value = s;
  }
}

// ── The render model ─────────────────────────────────────────────────────────
type Row =
  | { type: 'turnRule'; key: string; turn: number; stop: string; ctx: number | null }
  | { type: 'user'; key: string; anchor: string; n: number; time: string; text: string }
  | { type: 'agent'; key: string; time: string; text: string; streaming: boolean }
  | { type: 'thought'; key: string; text: string; streaming: boolean }
  | { type: 'tool'; key: string; tool: ToolCallPayload; live: boolean }
  | { type: 'permission'; key: string; perm: PermissionPayload };

interface TocItem {
  anchor: string;
  n: number;
  title: string;
}

function firstLine(s: string): string {
  return s.split('\n').map((l) => l.trim()).find(Boolean) ?? '';
}
function title(text: string): string {
  const f = firstLine(text) || '(no text)';
  return f.replace(/^[#>\-*`\s]+/, '').trim() || f;
}
function shortTime(ts: string): string {
  return ts.length >= 16 && ts[10] === 'T' ? ts.slice(11, 16) : ts;
}

const model = computed<{ rows: Row[]; toc: TocItem[] }>(() => {
  const rows: Row[] = [];
  const toc: TocItem[] = [];
  let n = 0;

  const sorted = [...blocks.values()].sort((a, b) => a.turn - b.turn || a.seq - b.seq);

  // Latest usage `used` at or before a given turn, for the turn-rule ctx figure.
  const usageBlocks = sorted.filter((b) => b.kind === 'usage');
  const usageAt = (turn: number): number | null => {
    let used: number | null = null;
    for (const u of usageBlocks) {
      if (u.turn <= turn) used = (u.payload as unknown as UsagePayload).used ?? used;
    }
    return used;
  };

  for (const b of sorted) {
    switch (b.kind) {
      case 'user_message': {
        n += 1;
        const anchor = `acp-turn-${b.turn}`;
        rows.push({
          type: 'user',
          key: blockKey(b.turn, b.seq),
          anchor,
          n,
          time: shortTime(b.created_at),
          text: (b.payload as unknown as UserMessagePayload).text ?? '',
        });
        toc.push({ anchor, n, title: title((b.payload as unknown as UserMessagePayload).text ?? '') });
        break;
      }
      case 'agent_message':
        rows.push({
          type: 'agent',
          key: blockKey(b.turn, b.seq),
          time: shortTime(b.created_at),
          text: (b.payload as unknown as AgentMessagePayload).text ?? '',
          streaming: false,
        });
        break;
      case 'thought':
        rows.push({
          type: 'thought',
          key: blockKey(b.turn, b.seq),
          text: (b.payload as unknown as ThoughtPayload).text ?? '',
          streaming: false,
        });
        break;
      case 'tool_call':
        rows.push({
          type: 'tool',
          key: blockKey(b.turn, b.seq),
          tool: b.payload as unknown as ToolCallPayload,
          live: false,
        });
        break;
      case 'permission_request':
        rows.push({
          type: 'permission',
          key: blockKey(b.turn, b.seq),
          perm: b.payload as unknown as PermissionPayload,
        });
        break;
      case 'turn_end':
        rows.push({
          type: 'turnRule',
          key: blockKey(b.turn, b.seq),
          turn: b.turn,
          stop: (b.payload as unknown as TurnEndPayload).stop_reason ?? 'end_turn',
          ctx: usageAt(b.turn),
        });
        break;
      // plan / mode_change / usage: not rendered inline in the transcript.
    }
  }

  // Trailing live content of the in-flight turn: a streaming shadow message /
  // thought, then any live tool calls (they trail because the backend flushes a
  // message buffer before a tool call starts).
  const lt = liveTurnNo.value;
  if (lt != null) {
    const thought = shadows.get(`${lt}:thought`);
    if (thought && thought.text) {
      rows.push({ type: 'thought', key: `shadow-${lt}-thought`, text: thought.text, streaming: true });
    }
    const msg = shadows.get(`${lt}:agent_message`);
    if (msg && msg.text) {
      rows.push({ type: 'agent', key: `shadow-${lt}-agent`, time: '', text: msg.text, streaming: true });
    }
    for (const t of liveTools.values()) {
      if (t.turn !== lt) continue;
      rows.push({
        type: 'tool',
        key: `live-${t.tool_call_id}`,
        tool: t as unknown as ToolCallPayload,
        live: t.status !== 'completed' && t.status !== 'failed' && t.status !== 'cancelled',
      });
    }
  }

  return { rows, toc };
});

// ── Thought / tool folds ─────────────────────────────────────────────────────
const open = ref<Set<string>>(new Set());
const isOpen = (k: string) => open.value.has(k);
function toggle(k: string) {
  const s = new Set(open.value);
  s.has(k) ? s.delete(k) : s.add(k);
  open.value = s;
}

// A tool call's kind glyph + a one-line content preview.
function toolGlyph(kind: string): string {
  return (
    { edit: '✎', execute: '⌗', delete: '✕', move: '⇄', read: '❏', search: '⌕', fetch: '⤓', think: '✳' } as Record<
      string,
      string
    >
  )[kind] ?? '•';
}
function toolText(tool: ToolCallPayload): string {
  return (tool.content ?? [])
    .map((c) => (c.type === 'text' ? c.text : `${c.path}\n-${c.old ?? ''}\n+${c.new}`))
    .join('\n')
    .trim();
}
function isAllow(kind: string): boolean {
  return kind.startsWith('allow');
}

// ── Working indicator + jump list scroll-spy ─────────────────────────────────
const convScroll = ref<HTMLElement | null>(null);
const activeAnchor = ref('');
function nearBottom(): boolean {
  const el = convScroll.value;
  if (!el) return true;
  return el.scrollHeight - el.scrollTop - el.clientHeight < 120;
}
function scrollToBottom() {
  const el = convScroll.value;
  if (el) el.scrollTop = el.scrollHeight;
}
function autoFollow() {
  if (nearBottom()) nextTick(scrollToBottom);
}
function goTo(anchor: string) {
  activeAnchor.value = anchor;
  convScroll.value
    ?.querySelector(`[data-anchor="${CSS.escape(anchor)}"]`)
    ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
}
</script>

<template>
  <div class="flex h-full flex-col">
    <p v-if="state === 'loading'" class="text-sm text-muted">Loading conversation…</p>
    <p v-else-if="state === 'error'" class="text-sm text-block">{{ errorMsg }}</p>

    <div v-else class="flex min-h-0 flex-1 gap-4">
      <!-- The transcript. -->
      <div
        ref="convScroll"
        data-testid="acp-conversation"
        class="acp-scroll min-h-0 flex-1 overflow-auto pb-8 pr-1"
      >
        <template v-for="row in model.rows" :key="row.key">
          <!-- Turn rule — a dashed hairline closing a turn. -->
          <div v-if="row.type === 'turnRule'" class="acp-turn-rule" data-testid="acp-turn-rule">
            <span>turn {{ row.turn + 1 }} · {{ row.stop }}<template v-if="row.ctx != null"> · {{ Math.round(row.ctx / 1000) }}k ctx</template></span>
          </div>

          <!-- YOU — the human turn. -->
          <section
            v-else-if="row.type === 'user'"
            :id="row.anchor"
            :data-anchor="row.anchor"
            class="acp-speaker"
          >
            <header class="acp-rule">
              <span class="acp-label text-accent">You</span>
              <span class="acp-time">{{ row.time }}</span>
            </header>
            <MarkdownView :id="id" path="" :source="row.text" />
          </section>

          <!-- AGENT — the model's prose. -->
          <section v-else-if="row.type === 'agent'" class="acp-speaker">
            <header class="acp-rule">
              <span class="acp-label">Agent</span>
              <span v-if="row.time" class="acp-time">{{ row.time }}</span>
            </header>
            <MarkdownView :id="id" path="" :source="row.text" />
          </section>

          <!-- Thinking — a faint fold. -->
          <div v-else-if="row.type === 'thought'" class="acp-thought">
            <button type="button" class="acp-fold-head" @click="toggle(row.key)">
              <span class="chev" :class="{ open: isOpen(row.key) }">▸</span>
              <span>thinking</span>
            </button>
            <p v-if="isOpen(row.key)" class="acp-thought-body">{{ row.text }}</p>
          </div>

          <!-- Tool call — a compact card. -->
          <div
            v-else-if="row.type === 'tool'"
            class="acp-tool"
            :class="{ 'acp-tool-failed': row.tool.status === 'failed' }"
            data-testid="acp-tool"
          >
            <div class="acp-tool-head">
              <span class="acp-tool-glyph">{{ toolGlyph(row.tool.tool_kind) }}</span>
              <span class="acp-tool-title">{{ row.tool.title || row.tool.tool_kind }}</span>
              <span v-if="row.live" class="acp-live">▸ live</span>
              <span v-else class="acp-tool-status">{{ row.tool.status }}</span>
            </div>
            <pre v-if="toolText(row.tool)" class="acp-payload">{{ toolText(row.tool) }}</pre>
          </div>

          <!-- Permission — the one interactive block. -->
          <div v-else-if="row.type === 'permission'" class="acp-perm" data-testid="acp-permission">
            <div class="acp-perm-label">Permission</div>
            <p class="acp-perm-title">{{ row.perm.title }}</p>
            <div v-if="row.perm.outcome" class="acp-perm-receipt" data-testid="acp-permission-receipt">
              {{ row.perm.outcome.option_id }} · {{ shortTime(row.perm.outcome.at) }}
            </div>
            <div v-else class="acp-perm-options">
              <button
                v-for="opt in row.perm.options"
                :key="opt.option_id"
                type="button"
                :class="isAllow(opt.kind) ? 'btn-primary' : 'btn-secondary'"
                class="px-2.5 py-1 text-xs"
                :disabled="answering.has(row.perm.request_id)"
                data-testid="acp-permission-option"
                @click="answer(row.perm, opt.option_id)"
              >
                {{ opt.name }}
              </button>
            </div>
          </div>
        </template>

        <!-- Optimistic (in-flight / queued) user messages. -->
        <section v-for="(o, i) in optimistic" :key="`opt-${i}`" class="acp-speaker" data-testid="acp-optimistic">
          <header class="acp-rule">
            <span class="acp-label text-accent">You</span>
            <span v-if="o.queued" class="acp-queued" data-testid="acp-queued">queued for next turn</span>
          </header>
          <MarkdownView :id="id" path="" :source="o.text" />
        </section>
      </div>

      <!-- Right rail: the user-turn jump list. -->
      <nav
        v-if="model.toc.length"
        class="hidden w-56 shrink-0 overflow-auto border-l border-line pl-3 lg:block"
        data-testid="acp-turns"
        aria-label="Turns"
      >
        <p class="mb-2 px-1 text-2xs font-medium uppercase tracking-wider text-faint">Turns</p>
        <ul class="space-y-0.5">
          <li v-for="t in model.toc" :key="t.anchor">
            <button
              type="button"
              class="acp-toc-item"
              :data-active="activeAnchor === t.anchor"
              @click="goTo(t.anchor)"
            >
              <span class="acp-toc-num">{{ t.n }}</span>
              <span class="truncate">{{ t.title }}</span>
            </button>
          </li>
        </ul>
      </nav>
    </div>

    <!-- Working indicator — a sage cue while a turn runs. -->
    <div v-if="turnLive" class="acp-working" data-testid="acp-working" role="status" aria-live="polite">
      <span>▶ working</span>
      <span v-if="liveTurnNo != null" class="acp-working-turn">· turn {{ liveTurnNo + 1 }}</span>
    </div>

    <!-- Composer. -->
    <form
      v-if="composerVisible"
      class="acp-composer"
      data-testid="acp-composer"
      @submit.prevent="submitPrompt"
    >
      <p v-if="sendError" class="mb-1.5 text-xs text-block" data-testid="acp-composer-error">{{ sendError }}</p>
      <textarea
        v-model="draft"
        rows="2"
        :disabled="sending"
        placeholder="Message the agent…  (Enter to send, Shift+Enter for a newline)"
        data-testid="acp-composer-input"
        class="acp-input"
        @keydown.enter.exact.prevent="submitPrompt"
      ></textarea>
      <div class="acp-composer-actions">
        <button
          v-if="turnLive"
          type="button"
          class="btn-secondary px-3 py-1 text-xs"
          data-testid="acp-composer-stop"
          :disabled="stopping"
          @click="stopTurn"
        >
          Stop
        </button>
        <button
          type="submit"
          class="btn-primary px-3 py-1 text-sm"
          data-testid="acp-composer-send"
          :disabled="sending || !draft.trim()"
        >
          {{ sending ? 'Sending…' : 'Send' }}
        </button>
      </div>
    </form>
  </div>
</template>

<style scoped>
/* Flatten MarkdownView's card into tight, left-aligned serif prose on the
   canvas — the transcript reads as printed dialogue, not stacked cards. */
.acp-scroll :deep(div:has(> .markdown-body)) {
  background: transparent;
  overflow: visible;
}
.acp-scroll :deep(.markdown-body) {
  max-width: none;
  margin: 0;
  padding: 0.125rem 0;
}

/* Speaker block: a hairline rule with a micro-caps label + mono time, serif
   prose beneath. No bubbles. */
.acp-speaker {
  scroll-margin-top: 0.5rem;
  margin-top: 1.25rem;
}
.acp-speaker:first-child {
  margin-top: 0;
}
.acp-rule {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  border-top: 1px solid var(--line);
  padding-top: 0.375rem;
  margin-bottom: 0.375rem;
}
.acp-label {
  font-family: var(--font-sans);
  font-size: 0.6875rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--muted);
}
.acp-time {
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  color: var(--faint);
  font-variant-numeric: tabular-nums;
}
.acp-queued {
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  color: var(--attn);
}

/* Thought fold — a faint italic-serif aside. */
.acp-thought {
  margin-top: 0.5rem;
}
.acp-fold-head {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  color: var(--faint);
  cursor: pointer;
}
.acp-fold-head:hover {
  color: var(--muted);
}
.chev {
  display: inline-block;
  transition: transform 0.12s ease;
}
.chev.open {
  transform: rotate(90deg);
}
.acp-thought-body {
  margin: 0.25rem 0 0 1.1rem;
  font-family: var(--font-serif);
  font-style: italic;
  font-size: 0.8125rem;
  line-height: 1.4;
  color: var(--faint);
  white-space: pre-wrap;
}

/* Tool card — mono header + a recessed payload. */
.acp-tool {
  margin-top: 0.625rem;
  border: 1px solid var(--line);
  border-radius: 0.375rem;
  overflow: hidden;
}
.acp-tool-failed {
  border-left: 2px solid var(--block-line);
}
.acp-tool-head {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.375rem 0.625rem;
  background: var(--surface);
  font-family: var(--font-mono);
  font-size: 0.75rem;
}
.acp-tool-glyph {
  color: var(--muted);
}
.acp-tool-title {
  color: var(--fg);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.acp-tool-status {
  margin-left: auto;
  color: var(--faint);
}
.acp-live {
  margin-left: auto;
  color: var(--ok);
}
.acp-payload {
  margin: 0;
  padding: 0.5rem 0.625rem;
  background: var(--code);
  color: var(--code-fg);
  font-family: var(--font-mono);
  font-size: 0.75rem;
  line-height: 1.15rem;
  white-space: pre-wrap;
  overflow-x: auto;
  max-height: 22rem;
}

/* Permission card — ochre rule + attention wash; the interface asking (sans). */
.acp-perm {
  margin-top: 0.75rem;
  border: 1px solid var(--line);
  border-left: 2px solid var(--attn-line);
  border-radius: 0.375rem;
  background: var(--attn-soft);
  padding: 0.625rem 0.75rem;
}
.acp-perm-label {
  font-family: var(--font-sans);
  font-size: 0.6875rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--attn);
}
.acp-perm-title {
  margin-top: 0.25rem;
  font-family: var(--font-serif);
  font-size: 0.875rem;
  color: var(--fg);
}
.acp-perm-options {
  margin-top: 0.5rem;
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.acp-perm-receipt {
  margin-top: 0.375rem;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  color: var(--muted);
}

/* Turn rule — a dashed hairline between turns. */
.acp-turn-rule {
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 1.25rem 0 0.25rem;
  border-top: 1px dashed var(--line);
  padding-top: 0.5rem;
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  color: var(--faint);
  font-variant-numeric: tabular-nums;
}

/* Working indicator. */
.acp-working {
  margin-top: 0.5rem;
  display: flex;
  align-items: center;
  gap: 0.375rem;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  color: var(--ok);
}
.acp-working-turn {
  color: var(--faint);
}

/* Composer. */
.acp-composer {
  margin-top: 0.75rem;
  border-top: 1px solid var(--line);
  padding-top: 0.75rem;
}
.acp-input {
  width: 100%;
  resize: vertical;
  max-height: 12rem;
  border-radius: 0.25rem;
  background: var(--input);
  padding: 0.5rem 0.625rem;
  font-family: var(--font-serif);
  font-size: 0.9375rem;
  outline: none;
}
.acp-input:focus {
  box-shadow: 0 0 0 1px var(--accent);
}
.acp-composer-actions {
  margin-top: 0.5rem;
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
}

/* Jump list. */
.acp-toc-item {
  display: flex;
  width: 100%;
  align-items: baseline;
  gap: 0.5rem;
  border-radius: 0.25rem;
  border-left: 2px solid transparent;
  padding: 0.1875rem 0.5rem;
  text-align: left;
  font-size: 0.75rem;
  line-height: 1.1rem;
  color: var(--muted);
  cursor: pointer;
}
.acp-toc-item:hover {
  background: color-mix(in srgb, var(--subtle) 55%, transparent);
  color: var(--fg);
}
.acp-toc-item[data-active='true'] {
  background: var(--subtle);
  border-left-color: var(--accent);
  color: var(--fg);
}
.acp-toc-num {
  flex: none;
  font-family: var(--font-mono);
  font-size: 0.625rem;
  color: var(--faint);
}
</style>
