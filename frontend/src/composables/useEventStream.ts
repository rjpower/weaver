import { ref, watch, onUnmounted } from 'vue'
import type { Ref } from 'vue'
import type { StreamEvent } from '../types'

export function useEventStream(issueId: Ref<string>, isRunning: Ref<boolean>) {
  const events = ref<StreamEvent[]>([])
  const connected = ref(false)
  let source: EventSource | null = null
  let lastSeq = 0

  function connect() {
    disconnect()
    source = new EventSource(`/api/issues/${issueId.value}/stream?after_seq=${lastSeq}`)
    connected.value = true

    source.addEventListener('message', (e: MessageEvent) => {
      try {
        const event: StreamEvent = JSON.parse(e.data)
        events.value = [...events.value, event]
        if (e.lastEventId) lastSeq = parseInt(e.lastEventId, 10)
      } catch {
        // ignore malformed events
      }
    })

    source.addEventListener('error', () => {
      connected.value = false
    })
  }

  function disconnect() {
    if (source) {
      source.close()
      source = null
    }
    connected.value = false
  }

  async function loadHistoricalEvents(id: string) {
    try {
      const resp = await fetch(`/api/issues/${id}/events?after_seq=0`)
      if (resp.ok) {
        const data = await resp.json()
        events.value = (data.events || []).map((e: any) => e as StreamEvent)
      }
    } catch {
      // leave events as-is
    }
  }

  watch([issueId, isRunning], async ([id, running]) => {
    if (running) {
      lastSeq = 0
      events.value = []
      connect()
    } else {
      disconnect()
      await loadHistoricalEvents(id as string)
    }
  }, { immediate: true })

  onUnmounted(disconnect)

  return { events, connected }
}
