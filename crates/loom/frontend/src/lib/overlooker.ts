import type { Overlooker, OverlookerTrigger, OverlookerScope } from '../types';

// The intervention ladder, calm → loud (mirrors weaver-core's CAPABILITIES).
// `observe` is implicit — always granted — so the create/edit forms only offer
// the explicit grants below it.
export const CAPABILITIES = ['observe', 'mark', 'escalate', 'nudge', 'interrupt', 'launch'] as const;
export const GRANTABLE_CAPABILITIES = ['mark', 'escalate', 'nudge', 'interrupt', 'launch'] as const;

// Final path segment of a repo root, for a short chip label.
export function repoLabel(path: string): string {
  return path.replace(/\/+$/, '').split('/').pop() || path;
}

// A one-line, human-readable summary of a trigger — what wakes a round.
// e.g. "cron 0 * * * *", "every 30m", "on attention=blocked", "on pr_red".
// An empty/unset trigger reads as "manual" (only fires on Run now).
export function triggerSummary(t: OverlookerTrigger | undefined | null): string {
  if (!t) return 'manual';
  if (t.cron) return `cron ${t.cron}`;
  if (t.every) return `every ${t.every}`;
  if (t.event) return t.level ? `on ${t.event}=${t.level}` : `on ${t.event}`;
  return 'manual';
}

// A one-line summary of the fleet scope a round surveys.
// e.g. "attention ≠ ok", "attention = blocked", or "whole fleet".
export function scopeSummary(s: OverlookerScope | undefined | null): string {
  if (!s || !s.attention) return 'whole fleet';
  const a = s.attention;
  return a.startsWith('!') ? `attention ≠ ${a.slice(1)}` : `attention = ${a}`;
}

// The judgement prompt a stock program runs, pulled out of `params`.
export function promptOf(o: Pick<Overlooker, 'params'>): string {
  const p = o.params?.prompt;
  return typeof p === 'string' ? p : '';
}
