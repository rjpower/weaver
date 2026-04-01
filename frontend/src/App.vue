<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()

type Theme = 'light' | 'dark' | 'system'
const theme = ref<Theme>('system')

function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyTheme(t: Theme) {
  const resolved = t === 'system' ? getSystemTheme() : t
  document.documentElement.setAttribute('data-theme', resolved)
}

onMounted(() => {
  const stored = localStorage.getItem('weaver-theme') as Theme | null
  if (stored && ['light', 'dark', 'system'].includes(stored)) {
    theme.value = stored
  }
  applyTheme(theme.value)

  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (theme.value === 'system') applyTheme('system')
  })
})

watch(theme, (t) => {
  localStorage.setItem('weaver-theme', t)
  applyTheme(t)
})

function cycleTheme() {
  const order: Theme[] = ['dark', 'light', 'system']
  const idx = order.indexOf(theme.value)
  theme.value = order[(idx + 1) % order.length]
}
</script>

<template>
  <div class="flex h-full">
    <aside class="w-[200px] max-md:w-14 bg-sidebar border-r border-border flex flex-col shrink-0 sticky top-0 h-screen">
      <div class="px-4 pt-5 pb-4 max-md:px-2 max-md:py-3 border-b border-border">
        <div class="font-extrabold text-[15px] text-text-primary tracking-tight">weaver</div>
        <div class="text-[11px] text-text-dim mt-0.5 max-md:hidden">task engine</div>
      </div>
      <nav class="flex-1 p-2 flex flex-col gap-0.5">
        <router-link to="/issues" custom v-slot="{ href, navigate, isActive }">
          <a :href="href" @click="navigate"
             :class="['flex items-center gap-2.5 px-3 py-2 max-md:justify-center max-md:px-2.5 text-[13px] font-medium rounded-md border-l-2 max-md:border-l-0 transition-colors',
                       isActive || route.path === '/'
                         ? 'text-accent border-accent bg-accent/5 max-md:bg-accent/10'
                         : 'text-text-secondary border-transparent hover:text-text-primary hover:bg-hover']">
            <svg class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
            </svg>
            <span class="max-md:hidden">Issues</span>
          </a>
        </router-link>
        <router-link to="/settings" custom v-slot="{ href, navigate, isActive }">
          <a :href="href" @click="navigate"
             :class="['flex items-center gap-2.5 px-3 py-2 max-md:justify-center max-md:px-2.5 text-[13px] font-medium rounded-md border-l-2 max-md:border-l-0 transition-colors',
                       isActive
                         ? 'text-accent border-accent bg-accent/5 max-md:bg-accent/10'
                         : 'text-text-secondary border-transparent hover:text-text-primary hover:bg-hover']">
            <svg class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 010 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 010-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28z"/>
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/>
            </svg>
            <span class="max-md:hidden">Settings</span>
          </a>
        </router-link>
      </nav>
      <div class="px-4 py-3 max-md:px-2 border-t border-border flex items-center justify-between">
        <div class="text-[11px] text-text-dim font-mono">v0.1.0</div>
        <button @click="cycleTheme"
                class="text-text-dim hover:text-text-primary bg-transparent border-none cursor-pointer p-1 rounded transition-colors hover:bg-hover"
                :title="'Theme: ' + theme">
          <svg v-if="theme === 'dark'" class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z"/>
          </svg>
          <svg v-else-if="theme === 'light'" class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z"/>
          </svg>
          <svg v-else class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1.007a3 3 0 01-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0115 18.257V17.25m6-12V15a2.25 2.25 0 01-2.25 2.25h-13.5A2.25 2.25 0 013 15V5.25A2.25 2.25 0 015.25 3h13.5A2.25 2.25 0 0121 5.25z"/>
          </svg>
        </button>
      </div>
    </aside>
    <main class="flex-1 overflow-auto">
      <router-view />
    </main>
  </div>
</template>
