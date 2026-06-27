<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue';
import { get, patch, listAgents } from '../api';
import type { AgentMetadata, SettingView } from '../types';
import ToggleSwitch from '../components/ToggleSwitch.vue';
import TokensPanel from '../components/TokensPanel.vue';
import AccountPanel from '../components/AccountPanel.vue';
import EnvPanel from '../components/EnvPanel.vue';

// In-page tabs: the setting registry (General), the agent env vars
// (Environment), API tokens, and the account / access surface (password,
// approved users, GitHub sign-in config).
type Tab = 'general' | 'env' | 'tokens' | 'account';
const tabs: { id: Tab; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'env', label: 'Environment' },
  { id: 'tokens', label: 'Tokens' },
  { id: 'account', label: 'Account' },
];
const tab = ref<Tab>('general');

// The server's canonical reply for both GET and PATCH /api/settings.
interface SettingsEnvelope {
  settings: SettingView[];
}

type SelectOption = { value: string; label: string };
type AgentSettingGroup = 'session' | 'concierge';

const settings = ref<SettingView[]>([]);
const agents = ref<AgentMetadata[]>([]);
// Per-key editable draft, keyed by setting key.
const drafts = ref<Record<string, string>>({});
const error = ref('');
const notice = ref('');
const busy = ref('');

// Settings grouped by their `group`, preserving registry order.
const groups = computed(() => {
  const out: { name: string; items: SettingView[] }[] = [];
  for (const s of settings.value) {
    let g = out.find((x) => x.name === s.group);
    if (!g) {
      g = { name: s.group, items: [] };
      out.push(g);
    }
    g.items.push(s);
  }
  return out;
});

const agentGroups: Record<AgentSettingGroup, { agent: string; model: string; effort: string }> = {
  session: { agent: 'agent.default', model: 'agent.model', effort: 'agent.effort' },
  concierge: {
    agent: 'concierge.runtime',
    model: 'concierge.model',
    effort: 'concierge.effort',
  },
};

function agentGroupFor(key: string): AgentSettingGroup | null {
  for (const [group, keys] of Object.entries(agentGroups) as [
    AgentSettingGroup,
    { agent: string; model: string; effort: string },
  ][]) {
    if (Object.values(keys).includes(key)) return group;
  }
  return null;
}

function isAgentRegistrySetting(s: SettingView): boolean {
  return agentGroupFor(s.key) !== null;
}

function agentsForGroup(group: AgentSettingGroup): AgentMetadata[] {
  return group === 'concierge'
    ? agents.value.filter((agent) => agent.supports_concierge)
    : agents.value;
}

function selectedAgent(group: AgentSettingGroup): AgentMetadata | undefined {
  const keys = agentGroups[group];
  const kind = drafts.value[keys.agent];
  return agentsForGroup(group).find((agent) => agent.kind === kind);
}

function registryOptions(s: SettingView): SelectOption[] {
  const group = agentGroupFor(s.key);
  if (!group) return [];
  const keys = agentGroups[group];
  if (s.key === keys.agent) {
    return agentsForGroup(group).map((agent) => ({ value: agent.kind, label: agent.label }));
  }

  const agent = selectedAgent(group);
  const choices = s.key === keys.model ? agent?.models ?? [] : agent?.efforts ?? [];
  return [
    { value: '', label: 'Default' },
    ...choices.map((choice) => ({ value: choice.id, label: choice.label })),
  ];
}

function sanitizeAgentDraft(group: AgentSettingGroup) {
  const keys = agentGroups[group];
  const availableAgents = agentsForGroup(group);
  if (!availableAgents.length) return;
  if (!availableAgents.some((agent) => agent.kind === drafts.value[keys.agent])) {
    drafts.value[keys.agent] = availableAgents[0].kind;
  }
  const agent = selectedAgent(group);
  if (!agent) return;
  if (
    drafts.value[keys.model] &&
    !agent.models.some((choice) => choice.id === drafts.value[keys.model])
  ) {
    drafts.value[keys.model] = '';
  }
  if (
    drafts.value[keys.effort] &&
    !agent.efforts.some((choice) => choice.id === drafts.value[keys.effort])
  ) {
    drafts.value[keys.effort] = '';
  }
}

function sanitizeAgentDrafts() {
  sanitizeAgentDraft('session');
  sanitizeAgentDraft('concierge');
}

async function load() {
  try {
    const [res, agentRes] = await Promise.all([
      get('/settings') as Promise<SettingsEnvelope>,
      listAgents(),
    ]);
    // Validate the shape before touching reactive state. A stale server — one
    // built before the settings endpoint existed — answers `{}`; assigning its
    // missing `settings` to the ref would crash the render (the `groups`
    // computed iterates it) and leave a blank page instead of this message.
    if (!Array.isArray(res?.settings)) {
      throw new Error('Unexpected /api/settings response — the server may be out of date.');
    }
    settings.value = res.settings;
    agents.value = agentRes.agents;
    drafts.value = Object.fromEntries(res.settings.map((s) => [s.key, s.value]));
    sanitizeAgentDrafts();
    error.value = '';
  } catch (e) {
    settings.value = [];
    error.value = (e as Error).message;
  }
}

function dirty(s: SettingView): boolean {
  return drafts.value[s.key] !== s.value;
}

async function act(key: string, fn: () => Promise<void>) {
  busy.value = key;
  error.value = '';
  notice.value = '';
  try {
    await fn();
  } catch (e) {
    error.value = (e as Error).message;
  } finally {
    busy.value = '';
  }
}

// Adopt a PATCH reply: refresh the canonical values, and resync only the
// changed key's draft so other in-progress edits are left untouched.
function adopt(res: SettingsEnvelope, changedKeys: string[]) {
  settings.value = res.settings;
  for (const changedKey of changedKeys) {
    const changed = res.settings.find((s) => s.key === changedKey);
    if (changed) drafts.value[changedKey] = changed.value;
  }
  sanitizeAgentDrafts();
}

function patchBody(s: SettingView, value: string | null): Record<string, string | null> {
  const group = agentGroupFor(s.key);
  if (!group) return { [s.key]: value };
  const keys = agentGroups[group];
  return {
    [keys.agent]: s.key === keys.agent ? value : drafts.value[keys.agent] ?? '',
    [keys.model]: s.key === keys.model ? value : drafts.value[keys.model] ?? '',
    [keys.effort]: s.key === keys.effort ? value : drafts.value[keys.effort] ?? '',
  };
}

// PATCH a single key — a value to set it, null to reset it to its default.
const apply = (s: SettingView, value: string | null, verb: string) =>
  act(s.key, async () => {
    const body = patchBody(s, value);
    const res = (await patch('/settings', body)) as SettingsEnvelope;
    adopt(res, Object.keys(body));
    notice.value = `${verb} ${s.label}.`;
  });

const save = (s: SettingView) => apply(s, drafts.value[s.key] ?? '', 'Saved');
const reset = (s: SettingView) => apply(s, null, 'Reset');

watch(
  () => [drafts.value['agent.default'], drafts.value['concierge.runtime'], agents.value.length],
  sanitizeAgentDrafts,
);

onMounted(load);
</script>

<template>
  <div class="max-w-3xl px-5 py-3">
    <div class="mb-2 flex min-h-7 items-center gap-2.5">
      <h1 class="text-2xs font-semibold uppercase tracking-wider text-muted">Settings</h1>
    </div>

    <!-- Tab bar: General (registry), Tokens, Account. -->
    <div class="mb-4 flex gap-1 border-b border-line">
      <button
        v-for="t in tabs"
        :key="t.id"
        :data-testid="`settings-tab-${t.id}`"
        class="-mb-px border-b-2 px-3 py-1.5 text-sm transition-colors"
        :class="
          tab === t.id
            ? 'border-accent text-fg'
            : 'border-transparent text-muted hover:text-fg'
        "
        @click="tab = t.id"
      >
        {{ t.label }}
      </button>
    </div>

    <EnvPanel v-if="tab === 'env'" />
    <TokensPanel v-else-if="tab === 'tokens'" />
    <AccountPanel v-else-if="tab === 'account'" />

    <div v-else>
      <p class="text-xs text-faint mb-3">
        Stored in the weaver database and shared by the server and CLI
        (<code>weaver config</code>).
      </p>

      <p v-if="error" class="mb-3 text-sm text-block">{{ error }}</p>
      <p v-if="notice" class="mb-3 text-sm text-accent">{{ notice }}</p>

      <!-- One bordered panel per group, hairline-divided rows — the same list
           anatomy as the fleet/issue boards, instead of free-floating cards. -->
      <div v-for="g in groups" :key="g.name" class="mb-5">
      <h2 class="text-2xs font-semibold uppercase tracking-wider text-muted mb-1.5">
        {{ g.name }}
      </h2>
      <div class="overflow-hidden rounded-md border border-line bg-surface">
        <section
          v-for="s in g.items"
          :key="s.key"
          class="border-b border-line px-3 py-2.5 last:border-0"
        >
          <div class="flex items-center justify-between gap-2">
            <label :for="s.key" class="text-sm font-medium">{{ s.label }}</label>
            <span class="font-mono text-2xs text-faint">{{ s.key }}</span>
          </div>
          <p class="text-xs text-muted mt-0.5 mb-2">{{ s.description }}</p>

          <div class="flex items-center gap-2">
            <div v-if="s.kind === 'bool'" class="flex flex-1 items-center gap-2 text-sm">
              <ToggleSwitch
                :id="s.key"
                :model-value="drafts[s.key] === 'true'"
                @update:model-value="drafts[s.key] = $event ? 'true' : 'false'"
              />
              <span class="text-xs text-muted">{{
                drafts[s.key] === 'true' ? 'Enabled' : 'Disabled'
              }}</span>
            </div>
            <select
              v-else-if="isAgentRegistrySetting(s)"
              :id="s.key"
              v-model="drafts[s.key]"
              :disabled="registryOptions(s).length <= 1 && s.key !== 'agent.default' && s.key !== 'concierge.runtime'"
              class="flex-1 rounded bg-input px-2 py-1 text-sm outline-none focus:ring-1 ring-accent"
            >
              <option v-for="opt in registryOptions(s)" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
            <select
              v-else-if="s.kind === 'enum'"
              :id="s.key"
              v-model="drafts[s.key]"
              class="flex-1 rounded bg-input px-2 py-1 text-sm outline-none focus:ring-1 ring-accent"
            >
              <option v-for="opt in s.options" :key="opt" :value="opt">{{ opt }}</option>
            </select>
            <input
              v-else
              :id="s.key"
              v-model="drafts[s.key]"
              :type="s.kind === 'int' ? 'number' : 'text'"
              :placeholder="s.default || '(empty)'"
              class="flex-1 rounded bg-input px-2 py-1 text-sm outline-none focus:ring-1 ring-accent"
              :class="{ 'font-mono': s.kind === 'string' }"
            />
            <button
              class="btn-primary px-2.5 py-1 text-xs"
              :disabled="busy === s.key || !dirty(s)"
              @click="save(s)"
            >
              Save
            </button>
            <button
              class="btn-secondary px-2.5 py-1 text-xs"
              :disabled="busy === s.key || s.is_default"
              :title="`Reset to default: ${s.default || '(empty)'}`"
              @click="reset(s)"
            >
              Reset
            </button>
          </div>
          <p class="mt-1.5 text-2xs text-faint">
            <span v-if="s.is_default">Using the default:</span>
            <span v-else>Customized · default is</span>
            <code class="ml-1 font-mono">{{ s.default || '(empty)' }}</code>
          </p>
        </section>
      </div>
      </div>

      <p v-if="!settings.length && !error" class="text-sm text-muted">Loading…</p>
    </div>
  </div>
</template>
