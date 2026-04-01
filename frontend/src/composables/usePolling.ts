import { onMounted, onUnmounted, ref } from 'vue'

export function usePolling(
  shouldPoll: () => boolean,
  pollFn: () => void,
  intervalMs: number,
) {
  const now = ref(Date.now())
  const lastUpdated = ref(Date.now())
  let pollTimer: ReturnType<typeof setInterval> | null = null
  let tickTimer: ReturnType<typeof setInterval> | null = null

  function start() {
    stop()
    pollTimer = setInterval(() => {
      if (shouldPoll()) {
        pollFn()
        lastUpdated.value = Date.now()
      }
    }, intervalMs)
    tickTimer = setInterval(() => { now.value = Date.now() }, 1000)
  }

  function stop() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null }
    if (tickTimer) { clearInterval(tickTimer); tickTimer = null }
  }

  function markUpdated() {
    lastUpdated.value = Date.now()
  }

  const lastUpdatedAgo = () => {
    const secs = Math.floor((now.value - lastUpdated.value) / 1000)
    if (secs < 5) return 'just now'
    return `${secs}s ago`
  }

  onMounted(start)
  onUnmounted(stop)

  return { now, lastUpdated, markUpdated, lastUpdatedAgo }
}
