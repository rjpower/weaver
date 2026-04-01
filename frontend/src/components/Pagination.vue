<script setup lang="ts">
defineProps<{
  currentPage: number
  totalPages: number
}>()

const emit = defineEmits<{
  page: [page: number]
}>()
</script>

<template>
  <div v-if="totalPages > 1" class="flex items-center justify-between px-4 py-3 border-t border-border">
    <button @click="emit('page', currentPage - 1)" :disabled="currentPage <= 1"
            class="text-xs font-semibold text-text-secondary bg-transparent border-none cursor-pointer px-2 py-1 rounded hover:text-text-primary hover:bg-hover disabled:text-text-dim disabled:cursor-not-allowed">
      Previous
    </button>
    <span class="text-xs text-text-dim">Page {{ currentPage }} of {{ totalPages }}</span>
    <button @click="emit('page', currentPage + 1)" :disabled="currentPage >= totalPages"
            class="text-xs font-semibold text-text-secondary bg-transparent border-none cursor-pointer px-2 py-1 rounded hover:text-text-primary hover:bg-hover disabled:text-text-dim disabled:cursor-not-allowed">
      Next
    </button>
  </div>
</template>
