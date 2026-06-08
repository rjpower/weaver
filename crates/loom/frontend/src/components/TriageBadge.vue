<script setup lang="ts">
import { computed } from 'vue';

// The overlooker's triage mark — a *second*, quieter badge that sits beside the
// agent's own AttentionBadge. It shares the amber/red hue family (semantic
// tokens, auto-swapping light/dark) but uses a soft outline treatment, never a
// loud fill: the agent's self-report is the one reserved loud signal, and this
// is an outside assessment that must read as secondary. The ⊙ glyph marks it as
// "watched". A `stale` mark — the session has moved on since the overlooker last
// looked — is faded and flagged in the tooltip, so an hour-old "looks stuck"
// never lies about a session that has since recovered.
const props = defineProps<{ level: string; note?: string; by?: string; stale?: boolean }>();

interface Style {
  label: string;
  cls: string;
}

const styles: Record<string, Style> = {
  ok: { label: 'OK', cls: 'text-faint ring-1 ring-inset ring-line' },
  attention: { label: 'Flagged', cls: 'bg-attn-soft text-attn ring-1 ring-inset ring-attn-line' },
  blocked: { label: 'Stuck', cls: 'bg-block-soft text-block ring-1 ring-inset ring-block-line' },
};

const style = computed(
  () => styles[props.level] ?? { label: props.level, cls: 'text-faint ring-1 ring-inset ring-line' },
);

const tooltip = computed(() => {
  const who = props.by && props.by !== 'manual' ? ` (${props.by})` : '';
  const base = props.note ? `Overlooker${who}: ${props.note}` : `Overlooker mark${who}`;
  return props.stale ? `${base} — stale, session has moved on` : base;
});
</script>

<template>
  <span
    :class="[style.cls, stale ? 'opacity-50' : '']"
    data-testid="triage-badge"
    :data-level="level"
    :data-stale="stale ? 'true' : 'false'"
    :title="tooltip"
    class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium uppercase tracking-wide"
  >
    <span aria-hidden="true">⊙</span>
    {{ style.label }}
  </span>
</template>
