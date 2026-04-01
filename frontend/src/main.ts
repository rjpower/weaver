import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'
import './styles.css'

import IssuesList from './views/IssuesList.vue'
import IssueDetail from './views/IssueDetail.vue'
import Settings from './views/Settings.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: IssuesList },
    { path: '/issues', component: IssuesList },
    { path: '/issues/:id', component: IssueDetail },
    { path: '/settings', component: Settings },
  ],
})

createApp(App).use(router).mount('#app')
