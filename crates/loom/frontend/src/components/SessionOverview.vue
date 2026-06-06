<script setup lang="ts">
import type { Session, WeaverEvent } from '../types';
import SessionActivity from './SessionActivity.vue';
import ScratchPanel from './ScratchPanel.vue';
import SessionPlan from './SessionPlan.vue';
import GithubStatus from './GithubStatus.vue';
import { timeAgo } from '../lib/time';

// The Overview tab: read-only context for a session, plus its lifecycle
// actions. Goal and the status message are authored by the AGENT (the launch
// prompt, `weaver set-status`) — shown here as prose, never as editable forms.
// The only writes on this page are acknowledging attention (the status strip),
// refreshing the GitHub snapshot, and lifecycle (Adopt / Archive / Remove).
const props = defineProps<{
  ws: Session;
  events: WeaverEvent[];
  format: (ev: WeaverEvent) => string;
  busy: string;
}>();

const emit = defineEmits<{ adopt: []; archive: []; remove: []; refreshGithub: [] }>();
</script>

<template>
  <div class="space-y-5">
    <!-- Plan — one surface for "what this branch is doing": the agent's launch
         goal at minimum, growing into the structured plan (tasks, diagram, live
         status projected from the issue ledger) once one is scaffolded. -->
    <SessionPlan :id="ws.id" :goal="ws.branch.goal" />

    <!-- GitHub — the branch's PR snapshot, polled server-side via `gh`. Shown
         only once a snapshot exists; the Refresh button forces a re-poll. -->
    <section class="rounded border border-line bg-surface p-4">
      <div class="mb-2 flex items-center justify-between">
        <div class="text-xs font-medium uppercase tracking-wide text-faint">GitHub</div>
        <button
          class="rounded bg-subtle px-2 py-1 text-xs text-muted hover:bg-subtle-hover disabled:opacity-50"
          :disabled="busy === 'refreshGithub'"
          @click="emit('refreshGithub')"
        >
          {{ busy === 'refreshGithub' ? 'Refreshing…' : 'Refresh' }}
        </button>
      </div>
      <template v-if="ws.branch.github">
        <GithubStatus :gh="ws.branch.github" />
        <p class="mt-2 text-xs text-faint">
          Snapshot {{ timeAgo(ws.branch.github.fetched_at) }}.
        </p>
      </template>
      <p v-else class="text-sm text-faint">
        No pull request found for this branch yet. loom polls automatically while
        the session is active.
      </p>
    </section>

    <!-- Activity — de-noised event feed (read-only context). -->
    <section class="rounded border border-line bg-surface p-4">
      <SessionActivity :events="events" :format="format" />
    </section>

    <!-- Scratch files. -->
    <ScratchPanel :id="ws.id" />

    <!-- Lifecycle actions — neutral; only Remove reads danger. Adopt carries a
         single suggested-action accent ring (it's the recovery path the strip
         flags); Archive is plain neutral; Remove is a quiet block-token ghost,
         loud only on hover. -->
    <section class="flex flex-wrap gap-2 rounded border border-line bg-surface p-4">
      <button
        v-if="ws.status === 'orphaned'"
        class="rounded bg-subtle px-3 py-1.5 text-sm text-accent ring-1 ring-inset ring-accent/30 hover:bg-subtle-hover"
        :disabled="busy === 'adopt'"
        @click="emit('adopt')"
      >
        {{ busy === 'adopt' ? 'Adopting…' : 'Adopt' }}
      </button>
      <button
        v-if="ws.status !== 'archived'"
        class="rounded bg-subtle px-3 py-1.5 text-sm text-fg hover:bg-subtle-hover"
        :disabled="busy === 'archive'"
        @click="emit('archive')"
      >
        {{ busy === 'archive' ? 'Archiving…' : 'Archive' }}
      </button>
      <button
        class="ml-auto rounded bg-transparent px-3 py-1.5 text-sm text-block ring-1 ring-inset ring-block-line hover:bg-block-soft"
        :disabled="busy === 'remove'"
        @click="emit('remove')"
      >
        Remove
      </button>
    </section>
  </div>
</template>
