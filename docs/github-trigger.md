# GitHub trigger (`@loom work on this`)

loom turns an issue comment into a session. Comment **`@loom work on this`** on a
GitHub issue and loom launches a session against that repo — seeded from the
issue — and replies on the issue with a link to the live session
(`On it — {base}/s/{id}`).

This is an internet-exposed, untrusted-input endpoint. Two gates protect it:
every delivery is verified cryptographically (HMAC), and the commenter is
authorized before anything is launched.

## How it works

GitHub delivers `issue_comment` events to `POST /api/github/webhook`. The
receiver, in order:

1. **Verifies the signature.** It recomputes HMAC-SHA256 over the raw request
   body with the webhook secret and constant-time-compares it to
   `X-Hub-Signature-256`. A missing or wrong signature is rejected with **401**.
2. **Dedupes** on `X-GitHub-Delivery`. A replayed or GitHub-retried delivery is a
   no-op, so a repeat never launches a second session.
3. **Filters** to `issue_comment` / `action == created`. Edits, deletions, other
   events, and the bot's own comments (set `github.bot_login`) are ignored.
4. **Matches the command.** The comment must *begin* with the trigger phrase
   (`github.trigger_phrase`, default `@loom work on this`), matched
   case-insensitively. Fixed phrase only — no free-text in v1.
5. **Authorizes the commenter.** They must be a known loom operator (their GitHub
   login is on the allowlist) **or** have write/admin permission on the repo
   (checked via `GET /repos/{owner}/{repo}/collaborators/{login}/permission`).
   Anyone else is silently ignored. A per-repo rate limit blunts comment spam.
6. **Resolves the repo** through the [managed repo store](#repo-allowlist) — only
   a registered repo is cloned and launched against — and creates the session,
   seeded with the issue title and body.
7. **Replies** on the issue with the session URL.

Everything past the signature check returns **200** whether or not it launched a
session (a non-trigger comment, a replay, an unauthorized commenter, an
unregistered repo, a rate-limited repo), so GitHub does not retry a delivery loom
deliberately ignored.

## Configure the webhook

Set a shared secret in loom's environment (it is held outside the settings
registry and never returned by `GET /api/settings`, like the OAuth client
secret):

```sh
LOOM_GITHUB_WEBHOOK_SECRET=<a long random string>
```

Add a webhook on the repo or org (**Settings → Webhooks → Add webhook**):

- **Payload URL** — `{base}/api/github/webhook`, where `{base}` is loom's public
  URL (the same one `auth.base_url` names).
- **Content type** — `application/json`.
- **Secret** — the same value as `LOOM_GITHUB_WEBHOOK_SECRET`.
- **Events** — "Let me select individual events" → **Issue comments** only.

Until `LOOM_GITHUB_WEBHOOK_SECRET` is set the endpoint rejects every delivery
(it cannot verify a signature without it).

### Repo allowlist

The trigger only acts on repos registered in the managed repo store, which
doubles as the clone allowlist. Register one first:

```sh
curl -X POST {base}/api/repos -H 'Authorization: Bearer $LOOM_TOKEN' \
  -H 'content-type: application/json' -d '{"repo":"owner/name"}'
```

A comment on an unregistered repo launches nothing.

### Optional settings

- `github.trigger_phrase` — the phrase a comment must begin with
  (default `@loom work on this`). Settable in **Settings → GitHub** or
  `weaver config set github.trigger_phrase "…"`.
- `github.bot_login` (or `LOOM_GITHUB_BOT_LOGIN`) — a GitHub login whose own
  comments are ignored, so a bot account can post without re-triggering itself.

## Authentication and the planned hardening

v1 authenticates the webhook with a shared secret and acts on GitHub through the
ambient `GH_TOKEN` (the `gh` CLI) — the permission check and the reply both run
as that token's identity. The planned hardening is a **GitHub App**: per-repo
installation as the access allowlist, short-lived installation tokens for the API
calls and the reply, and a distinct bot identity for attribution. The webhook
receiver and its security controls (HMAC, idempotency, authorization) are
unchanged by that move.
