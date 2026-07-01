<script setup lang="ts">
import { ref, shallowRef, watch, onMounted, nextTick, h, Fragment } from 'vue';
import { useRouter } from 'vue-router';
import { parseMarkdown, loadSanitizer, type RenderContext } from '../markdown';
import { renderTokens, type RenderCtx } from '../markdown-render';
import type Token from 'markdown-it/lib/token.mjs';
import type { IssueRefStatus } from '../types';

// Rendered Markdown preview, as a real Vue tree — markdown-it tokens walked into
// vnodes (`markdown-render.ts`), not an `innerHTML` blob. GitHub-flavoured output
// (tables, task lists, inline images) plus the smartdoc projection (`#N` issue
// refs → live status chips, `artifact:<name>` → deep links); fenced code and
// mermaid are the CodeBlock / MermaidBlock leaf components, which lazy-load their
// heavy renderer. This component just parses + hosts; the comment layer lives in
// ArtifactDocument (which renders the same tokens and interleaves threads).
const props = defineProps<{
  /** Session id — used to resolve relative images to the raw-bytes endpoint and
   *  to build `artifact:` deep links. */
  id: string;
  /** Repo-relative path of the markdown file (its directory anchors images). */
  path: string;
  /** Raw markdown source. */
  source: string;
  /** Live status for `#N` issue references, keyed by id-as-string — the
   *  projection map. For an artifact pass `ArtifactView.refs.issues`; for the
   *  goal pass a client-built map from the session's issues. Absent → refs render
   *  as plain text. */
  refs?: Record<string, IssueRefStatus>;
}>();

const router = useRouter();

const emit = defineEmits<{ rendered: [el: HTMLElement | null] }>();

const body = ref<HTMLElement | null>(null);
const error = ref('');
// Tokens + render context drive the vnode body. shallowRef: they're replaced
// wholesale each build, never mutated in place.
const tokens = shallowRef<Token[]>([]);
const ctx = shallowRef<RenderCtx | null>(null);

function renderContext(): RenderContext {
  return { sessionId: props.id, filePath: props.path, refs: props.refs };
}

// The vnode body, recreated on every render so no vnode is ever reused across
// renders (fresh nodes; Vue keeps child component state via keys). Reads the
// reactive `tokens`/`ctx`, so it re-renders when a build lands.
const RenderedBody = () =>
  ctx.value ? h(Fragment, renderTokens(tokens.value, ctx.value)) : null;

// A monotonic token guards against an out-of-order finish: if the source changes
// while an async parse is mid-flight, only the latest run paints.
let runId = 0;

async function build() {
  const mine = ++runId;
  try {
    const [toks, sanitize] = await Promise.all([
      parseMarkdown(props.source, renderContext()),
      loadSanitizer(),
    ]);
    if (mine !== runId) return;
    ctx.value = { ...renderContext(), sanitize };
    tokens.value = toks;
    error.value = '';
    // Let the fresh tree paint, then tell any host the DOM is ready.
    await nextTick();
    if (mine !== runId) return;
    emit('rendered', body.value);
  } catch (e) {
    if (mine === runId) error.value = (e as Error).message;
  }
}

watch(() => [props.source, props.path, props.refs], build, { deep: true });
onMounted(build);

// Intercept in-page `#` links (heading permalinks, in-doc TOC links): scroll the
// matching element into view rather than letting the browser change the hash,
// which would clobber the SPA's hash-based router. smartdoc chips/links (carrying
// `data-issue` / `data-artifact`) are internal SPA routes — push them through the
// router so they navigate in-app instead of a full reload.
function onClick(e: MouseEvent) {
  const anchor = (e.target as HTMLElement).closest('a');
  if (!anchor) return;
  const href = anchor.getAttribute('href');
  if (!href) return;
  if (anchor.hasAttribute('data-issue') || anchor.hasAttribute('data-artifact')) {
    e.preventDefault();
    router.push(href);
    return;
  }
  if (!href.startsWith('#')) return;
  e.preventDefault();
  const id = decodeURIComponent(href.slice(1));
  body.value?.querySelector(`[id="${CSS.escape(id)}"]`)?.scrollIntoView({
    behavior: 'smooth',
    block: 'start',
  });
}

// Let a parent (ArtifactDocument's sibling uses, or a host that measures the
// rendered prose) reach the rendered <article> directly.
defineExpose({ body });
</script>

<template>
  <div class="h-full w-full overflow-auto bg-surface">
    <p v-if="error" class="m-4 rounded border border-block-line bg-block-soft p-3 text-sm text-block">
      {{ error }}
    </p>
    <article ref="body" class="markdown-body mx-auto max-w-3xl px-6 py-5" @click="onClick">
      <RenderedBody />
    </article>
  </div>
</template>
