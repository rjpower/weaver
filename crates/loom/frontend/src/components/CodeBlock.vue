<script setup lang="ts">
import { ref, watch, onMounted } from 'vue';

// A single fenced code block: `<pre class="hljs"><code>…</code></pre>`, syntax-
// highlighted by highlight.js. The highlighter is heavy, so it's lazy-loaded on
// mount (kept out of the main chunk) and re-run whenever the source or language
// changes. Until it resolves — and whenever there's no known language — the
// escaped source shows as plain text so there's no flash of nothing. hljs output
// is trusted HTML (it escapes the content and only wraps tokens in `hljs-*`
// spans), so it's assigned via innerHTML. Mermaid isn't handled here: the caller
// routes `lang === 'mermaid'` to MermaidBlock instead.
const props = defineProps<{
  /** Raw code source (markdown-it fence content — includes a trailing newline). */
  code: string;
  /** Fenced info-string language, e.g. `ts`. Absent → render as plain text. */
  lang?: string;
}>();

const codeEl = ref<HTMLElement | null>(null);

// A monotonic token guards against an out-of-order finish: if the source/lang
// changes (or the block unmounts) while the async import is mid-flight, only the
// latest run gets to paint.
let runId = 0;

async function highlight() {
  const el = codeEl.value;
  if (!el) return;
  const mine = ++runId;
  const lang = props.lang;
  // markdown-it's fence content carries a trailing newline; drop a single one so
  // the block has no blank last line.
  const src = props.code.replace(/\n$/, '');
  // Show the escaped source immediately (textContent escapes natively) so there's
  // no flash before highlight.js loads — and it's the final render when there's
  // no highlightable language.
  el.textContent = src;
  if (!lang) return;
  const hljs = (await import('highlight.js')).default;
  // Superseded by a newer run, or unmounted → don't touch the (possibly stale)
  // node.
  if (mine !== runId || codeEl.value !== el) return;
  if (hljs.getLanguage(lang)) {
    try {
      el.innerHTML = hljs.highlight(src, { language: lang, ignoreIllegals: true }).value;
    } catch {
      // Leave the escaped plain text in place.
    }
  }
}

watch(() => [props.code, props.lang], highlight);
onMounted(highlight);
</script>

<template>
  <pre class="hljs"><code ref="codeEl" :class="lang ? `language-${lang}` : null"></code></pre>
</template>
