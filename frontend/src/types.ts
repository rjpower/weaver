export type IssueStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'validation_failed'
  | 'blocked'
  | 'awaiting_review'

export interface UsageSummary {
  input_tokens: number
  output_tokens: number
  cost_usd: number
  model: string | null
  attempts: number
}

export interface Issue {
  id: string
  title: string
  body: string
  status: IssueStatus
  context: Record<string, unknown>
  dependencies: string[]
  num_tries: number
  max_tries: number
  parent_issue_id: string | null
  tags: string[]
  priority: number
  channel_kind: string | null
  origin_ref: string | null
  user: string | null
  error: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
  usage: UsageSummary | null
}

export interface OrganizedIssue extends Issue {
  _isChild: boolean
  _depth: number
}

export interface IssueListResponse {
  issues: Issue[]
  total: number
}

export interface Comment {
  id: number
  issue_id: string
  author: string
  body: string
  tag: string | null
  created_at: string
}

export interface DiffResponse {
  diff: string
  branch: string | null
  base: string
  work_dir: string | null
  files_changed: string[]
  error: string | null
}

export interface FileContentResponse {
  path: string
  content: string
}

export interface PrInfoResponse {
  branch: string | null
  compare_url: string | null
}

export interface CreateIssueRequest {
  title: string
  body: string
  tags: string[]
  priority: number
}

export interface ReviseRequest {
  feedback: string
  tags?: string[]
}

export interface ApproveRequest {
  comment?: string
}

export interface StreamEvent {
  kind: string
  session_id?: string
  model?: string
  text?: string
  tool?: string
  input?: string
  output?: string
  result?: string
  input_tokens?: number
  output_tokens?: number
  cost_usd?: number
  message?: string
}
