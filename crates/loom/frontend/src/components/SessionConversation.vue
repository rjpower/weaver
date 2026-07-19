<script setup lang="ts">
import { computed } from 'vue';
import type { Session } from '../types';
import TerminalConversation from './TerminalConversation.vue';
import AcpConversation from './AcpConversation.vue';

// The Conversation surface picks its data source by the session's execution
// backend. An ACP session (`protocol='acp'`) renders from the live chat journal
// (`/chat` + `/chat/stream`); a terminal session keeps the iris scrape path
// (`/conversation`) untouched. One prop, one seam — everything backend-specific
// lives in the two child components.
const props = defineProps<{ session: Session }>();
const isAcp = computed(() => props.session.protocol === 'acp');
</script>

<template>
  <AcpConversation v-if="isAcp" :session="session" />
  <TerminalConversation v-else :session="session" />
</template>
