<script setup lang="ts">
import { ref, watch, onMounted, onBeforeUnmount } from 'vue';
import { theme } from '../theme';

// A single mermaid diagram, rendered to an inline `<svg>`. mermaid + DOMPurify
// are heavy, so they're lazy-loaded on mount (kept out of the main chunk) and
// re-run whenever the source or the app theme changes, so the diagram tracks the
// light/dark palette. A diagram that fails to parse shows an inline error note
// rather than throwing — and `suppressErrorRendering` keeps mermaid from leaking
// its "bomb" error graphic into `document.body` (see below).
const props = defineProps<{
  /** Mermaid diagram source. */
  code: string;
}>();

const host = ref<HTMLElement | null>(null);

// Unique render id per pass, so mermaid's temp DOM nodes never collide. Kept
// module-level (not Date.now/Math.random) so ids are stable and deterministic.
let mermaidSeq = 0;

// Mermaid's stock `default`/`dark` themes paint nodes lavender-purple, which
// clashes with loom's neutral-slate + single-blue palette. Drive mermaid's
// `base` theme with loom's own tokens instead so diagrams read as part of the
// same UI in both palettes. Hexes mirror the slate/blue values in styles.css.
function mermaidThemeVariables(dark: boolean): Record<string, string> {
  return dark
    ? {
        background: '#1e293b', // surface (slate-800)
        primaryColor: '#0f172a', // canvas (slate-900) — node fill
        primaryBorderColor: '#475569', // slate-600
        primaryTextColor: '#f1f5f9', // slate-100
        lineColor: '#64748b', // slate-500 — edges
        secondaryColor: '#334155', // slate-700
        tertiaryColor: '#0f172a',
      }
    : {
        background: '#ffffff',
        primaryColor: '#f1f5f9', // slate-100 — node fill
        primaryBorderColor: '#cbd5e1', // slate-300
        primaryTextColor: '#0f172a', // slate-900
        lineColor: '#94a3b8', // slate-400 — edges
        secondaryColor: '#e2e8f0', // slate-200
        tertiaryColor: '#f8fafc',
      };
}

// A monotonic token guards against an out-of-order finish: a re-render (source or
// theme flip) while one is in flight — or the component unmounting mid-render —
// bumps the token so only the latest run paints, and never into a detached node.
let runId = 0;

async function render() {
  const el = host.value;
  if (!el) return;
  const mine = ++runId;
  const dark = theme.value === 'dark';
  const [mermaid, purifyMod] = await Promise.all([
    import('mermaid').then((m) => m.default),
    import('dompurify'),
  ]);
  if (mine !== runId || host.value !== el) return;
  const DOMPurify = purifyMod.default;
  // initialize is idempotent-ish but theme can change with the app theme, so
  // re-apply it each pass.
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    theme: 'base',
    themeVariables: mermaidThemeVariables(dark),
    fontFamily: 'inherit',
    // Throw on a bad diagram instead of drawing mermaid's "bomb" error graphic.
    // The default renders that graphic into a temp node mermaid appends to
    // `document.body` and only removes on success — so a failed parse leaks the
    // bomb into the page body, where it survives route changes and shows up at
    // the bottom of every view. Suppressing it makes `render` reject cleanly
    // (mermaid removes its temp node), and the catch below shows our own inline
    // error note instead.
    suppressErrorRendering: true,
  });

  const id = `weaver-mermaid-${mermaidSeq++}`;
  try {
    const { svg } = await mermaid.render(id, props.code);
    if (mine !== runId || host.value !== el) return;
    // Mermaid already sanitises under `securityLevel: 'strict'`, but the SVG is
    // derived from DOM text, so pass it through DOMPurify before innerHTML for
    // defence-in-depth (and to keep the flow provably XSS-safe).
    //
    // Mermaid draws node/cluster labels as HTML inside `<foreignObject>`. That
    // only survives sanitising if DOMPurify is told `foreignobject` is an HTML
    // integration point (otherwise its XHTML children are dropped as stray SVG
    // and every label renders blank). These options mirror mermaid's own
    // internal sanitise call, so we keep its structure verbatim while still
    // stripping scripts/handlers.
    const sanitized = DOMPurify.sanitize(svg, {
      ADD_TAGS: ['foreignobject'],
      ADD_ATTR: ['dominant-baseline'],
      HTML_INTEGRATION_POINTS: { foreignobject: true },
    });
    el.innerHTML = `<div class="mermaid-diagram">${sanitized}</div>`;
  } catch (e) {
    if (mine !== runId || host.value !== el) return;
    // Build the note imperatively so the (untrusted) error message is inserted as
    // text, never parsed as HTML.
    const err = document.createElement('div');
    err.className = 'mermaid-error';
    err.textContent = `Could not render mermaid diagram: ${(e as Error).message}`;
    el.replaceChildren(err);
  }
}

// Bump the token so any in-flight render bails rather than writing into the
// detached host.
onBeforeUnmount(() => {
  runId++;
});

watch([() => props.code, theme], render);
onMounted(render);
</script>

<template>
  <div ref="host"></div>
</template>
