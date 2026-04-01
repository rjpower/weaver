export async function api<T>(path: string): Promise<T> {
  const resp = await fetch(path)
  if (!resp.ok) throw new Error(`API ${resp.status}: ${path}`)
  return resp.json()
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T | null> {
  const opts: RequestInit = { method: 'POST' }
  if (body !== undefined) {
    opts.headers = { 'Content-Type': 'application/json' }
    opts.body = JSON.stringify(body)
  }
  const resp = await fetch(path, opts)
  if (!resp.ok) throw new Error(`API ${resp.status}: ${path}`)
  const text = await resp.text()
  return text ? JSON.parse(text) : null
}

export async function apiPut<T>(path: string, body: unknown): Promise<T | null> {
  const resp = await fetch(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!resp.ok) throw new Error(`API ${resp.status}: ${path}`)
  const text = await resp.text()
  return text ? JSON.parse(text) : null
}

export async function apiDelete(path: string): Promise<void> {
  const resp = await fetch(path, { method: 'DELETE' })
  if (!resp.ok && resp.status !== 404) throw new Error(`API ${resp.status}: ${path}`)
}
