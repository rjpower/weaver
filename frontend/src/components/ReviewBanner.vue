<script setup lang="ts">
defineProps<{
  approving: boolean
}>()

const emit = defineEmits<{
  approve: []
  requestChanges: []
}>()
</script>

<template>
  <div class="flex items-center justify-between px-4 py-3 bg-review/[0.08] border border-review/20 rounded-lg mb-4">
    <span class="text-[13px] font-medium text-review">This issue is awaiting your review</span>
    <div class="flex gap-2">
      <button @click="emit('approve')" :disabled="approving"
              class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-success text-white hover:opacity-85 disabled:opacity-40 disabled:cursor-not-allowed">
        {{ approving ? 'Approving...' : 'Approve' }}
      </button>
      <button @click="emit('requestChanges')"
              class="inline-flex items-center justify-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold rounded-md border-none cursor-pointer bg-warning text-black hover:opacity-85">
        Request Changes
      </button>
    </div>
  </div>
</template>
