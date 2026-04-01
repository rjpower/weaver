import type { Issue, IssueStatus, OrganizedIssue } from './types'

export function relativeTime(iso: string | null): string {
  if (!iso) return ''
  const seconds = Math.floor((Date.now() - new Date(iso).getTime()) / 1000)
  if (seconds < 0) return 'just now'
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

export function duration(start: string | null, end: string | null): string {
  if (!start) return ''
  const s = new Date(start).getTime()
  const e = end ? new Date(end).getTime() : Date.now()
  const secs = Math.floor((e - s) / 1000)
  if (secs < 60) return `${secs}s`
  const mins = Math.floor(secs / 60)
  const remSecs = secs % 60
  if (mins < 60) return `${mins}m ${remSecs}s`
  const hours = Math.floor(mins / 60)
  const remMins = mins % 60
  return `${hours}h ${remMins}m`
}

export function truncate(text: string | null, len: number): string {
  if (!text) return ''
  return text.length > len ? text.substring(0, len) + '...' : text
}

const STATUS_CLASSES: Record<IssueStatus, string> = {
  pending: 'bg-warning/10 text-warning',
  running: 'bg-info/10 text-info',
  completed: 'bg-success/10 text-success',
  failed: 'bg-error/10 text-error',
  validation_failed: 'bg-error/10 text-error',
  blocked: 'bg-warning/10 text-warning',
  awaiting_review: 'bg-review/10 text-review',
}

export function statusClasses(status: IssueStatus): string {
  return STATUS_CLASSES[status] ?? 'bg-success/10 text-success'
}

export function organizeByParent(issues: Issue[]): OrganizedIssue[] {
  // Backend returns issues in tree order (parent then descendants DFS).
  // Compute depth by tracking each issue's parent chain.
  const depthMap = new Map<string, number>()
  const result: OrganizedIssue[] = []

  for (const issue of issues) {
    let depth = 0
    if (issue.parent_issue_id && depthMap.has(issue.parent_issue_id)) {
      depth = depthMap.get(issue.parent_issue_id)! + 1
    }
    depthMap.set(issue.id, depth)
    result.push({ ...issue, _isChild: depth > 0, _depth: depth })
  }

  return result
}
