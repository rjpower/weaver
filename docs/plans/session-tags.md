# Plan: session tags (unify `attention` + `triage`)

> Status: **in progress**. This is a plan, not current-state docs. Once shipped,
> fold the surviving design notes into `docs/ARCHITECTURE.md` and
> `crates/weaver-core/WEAVER.md` and delete this file.

## Why

A session carried two near-identical status axes — the agent's `attention`
(self-report) and an overlooker's `triage` (outside assessment) — drawn from the
same `ok | attention | blocked` ladder, differing only in *who authored it*.
That is two columns, two events, two badges, two CLI verbs for one shape:
`(level, note, author, timestamp)`.

Collapse both into one general mechanism: a **tag** on a branch. `attention` and
`triage` become two *well-known keys*; new axes (priority, needs-rebase, …) cost
zero schema. The visible surface collapses to one resolved attention signal plus
quiet deletable pills.

## Data model

A `tags` table in the shared (weaver-core) DB. One row per `(branch_id, key)` —
single-valued, upsert:

```sql
CREATE TABLE IF NOT EXISTS tags (
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL DEFAULT '',
    note      TEXT NOT NULL DEFAULT '',
    set_by    TEXT NOT NULL DEFAULT '',
    set_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (branch_id, key)
);
CREATE INDEX IF NOT EXISTS idx_tags_key ON tags(key, value);
```

Rules:

- **Absence is the calm/default state.** There is no stored `ok`. Setting
  attention to `ok` *deletes* the tag. This structurally enforces "ok ⇒ no
  badge".
- **`branch.description` stays.** It is the agent's prose status message, shown
  even when calm. Only the attention *level* moves to a tag. `weaver set-status
  ok "msg"` updates `description` **and** clears the `attention` tag.
- **Well-known keys** (registry in `weaver-core/src/tags.rs`):
  - `attention` — author: the agent. Stored values: `attention | blocked`.
    Loud.
  - `triage` — author: an overlooker (or `manual`). Stored values: `attention |
    blocked`. Loud. Its `note`/`set_by`/`set_at` carry the mark's reason,
    attribution, and staleness anchor.
  - Any other key is free-form, quiet, single-valued.
- **Staleness** is generic: a tag is stale when `set_at < last_activity_at`
  (the session moved on since it was set).

## Events

One event kind, `tag`, with `data = {"key","value","note","by"}` (empty `value`
= the tag was cleared). The monitor re-broadcasts `tag` events (the daemon-less
CLI writes them without the bus), exactly as it did for `attention`/`triage`.

Overlooker triggers are unchanged on disk: `{event:"attention", level:"blocked"}`.
The dispatcher maps a `tag` event's `key` → the match `kind` and `value` → the
match `level`, so `Trigger::matches_event` keeps working verbatim.

## Migration `0005_tags.sql`

Create the table + index, move existing non-ok marks in, drop the old columns:

```sql
-- (create table + index as above)
INSERT INTO tags (branch_id, key, value, note, set_by, set_at)
  SELECT id, 'attention', attention, '', 'agent', updated_at
  FROM branches WHERE attention IN ('attention','blocked');
INSERT INTO tags (branch_id, key, value, note, set_by, set_at)
  SELECT id, 'triage', triage_level, triage_note, triage_by,
         COALESCE(triage_at, updated_at)
  FROM branches WHERE triage_level IN ('attention','blocked');  -- drop 'ok' marks
ALTER TABLE branches DROP COLUMN attention;
ALTER TABLE branches DROP COLUMN triage_level;
ALTER TABLE branches DROP COLUMN triage_note;
ALTER TABLE branches DROP COLUMN triage_by;
ALTER TABLE branches DROP COLUMN triage_at;
```

The migration runs on the dedicated single-connection pool before the shared
pool opens (existing weaver-core db.rs pattern), so DROP COLUMN cannot leave a
connection with a stale cached schema.

## Resolved attention signal (UI)

`effectiveAttention(session)` = the louder of the agent's `attention` tag and the
non-stale `triage` tag, carrying attribution (which key/`set_by` raised it). One
loud badge per row. The agent saying `ok` while an overlooker says `attention`
surfaces as "needs attention (raised by <overlooker>)". Every non-loud tag
renders as a quiet pill with a `×` that clears it (`DELETE …/tags/<key>`).

## Tasks

Each task is one subagent; verify with the listed command; keep the PR green.

### T1 — Foundation: `weaver-core` + `weaver-api`
- `0005_tags.sql` + register in `migrations.rs`; add a migration test (data moved,
  columns dropped, idempotent).
- `weaver-core/src/tags.rs`: `Tag` struct; `set`/`clear`/`get`/`list`; registry
  (`ATTENTION_KEY`, `TRIAGE_KEY`, `is_loud`, valid stored values, `validate`).
  `pub mod tags;` in lib.rs.
- `branch.rs`: drop `attention`, `triage_*` fields + `set_attention`,
  `set_triage`, `*_LEVELS`, `is_valid_*`, `DEFAULT_ATTENTION`. Keep
  `description`/`set_description`. Move the "separate axis" test into `tags.rs`.
- `events.rs`: a `record_tag` helper centralizing the `{key,value,note,by}` shape.
- `db.rs`: update the test that decodes triage columns.
- `weaver-api/src/dto.rs`: `TagView{key,value,note,set_by,set_at}`; drop
  attention/triage_* from `BranchView`/`SessionView`, add `tags: Vec<TagView>`;
  drop `attention` from `PatchSessionReq`; replace `TriageReq` with `TagReq`.
- `weaver-api/src/client.rs`: `set_tag`/`clear_tag` (+ keep a `mark` convenience
  that writes the `triage` tag). `capability.rs` unchanged.
- `weaver-core/WEAVER.md`: describe the tags model (current state).
- Verify: `cargo test -p weaver-core -p weaver-api`.

### T2 — loom backend + integration tests
- `web.rs`: `PUT /sessions/{id}/tags/{key}` (upsert) + `DELETE
  /sessions/{id}/tags/{key}` (clear); drop `triage_session` and the attention
  branch of `patch_session`; `session_view` includes `tags`; router wiring; the
  `tag` event emit.
- `overlooker.rs`: `mark` writes the `triage` tag + a `tag` event; fallback rule
  reads the `attention` tag; `scope.admits` is fed the resolved `attention` tag
  value (absent ⇒ `ok`); dispatcher maps `tag` event `key`/`value` → match
  `kind`/`level`.
- `monitor.rs`: re-broadcast `tag` events (replacing the attention/triage cases);
  keep the `stale` emit.
- Integration tests: `branches.rs`, `overlookers.rs`, `typed_client.rs`,
  `archive.rs`, `hook_monitor.rs`.
- Verify: `cargo test -p loom`.

### T3 — weaver CLI + tests
- `weaver.rs`: `set-status` writes/clears the `attention` tag + `tag` event;
  remove the `Triage` subcommand; add a general `tag` group (`set`/`rm`/`ls`);
  `readme`/status display and the `issue wait` attention read both resolve the
  `attention` tag.
- `weaver/tests/agent_cli.rs`.
- Verify: `cargo test -p weaver`.

### T4 — frontend + e2e
- `types.ts`: drop attention/triage_*; add `tags: Tag[]`.
- `sessionState.ts`: `levelOf`/`triageOf` read tags; add `effectiveAttention`
  and generic `tagStale`.
- Collapse `AttentionBadge` + `TriageBadge` into one resolved attention signal
  with attribution; add a `TagPill`/tag-list for quiet deletable tags.
- `sessionActions.ts`: acknowledge ⇒ delete the `attention` tag; pill `×` ⇒
  delete tag. Update `SessionList`, `SessionDetail`, `SessionPageHeader`,
  `SessionActivity`, `SessionOverview`, `Overlookers`, `OverlookerDetail`,
  `overlooker.ts`.
- e2e: `list.spec`, `detail.spec`, `overlookers.spec`, `status-hook.spec`.
- Verify: frontend build + `playwright test`.

### T5 — weaver-py + docs
- `weaver-py`: reads expose `tags`; `set_tag`/`clear_tag` writes; `.pyi`,
  `examples/fleet_status.py`, tests, README.
- Docs: `ARCHITECTURE.md`, `docs/plans/overlooker.md`, `structured-projects.md`,
  `browser-terminal.md`, `lint.md` — describe tags as current state.
- Verify: `maturin build` + `pytest`; docs read-through.

### Final (me)
- Full `cargo test` + e2e; visual review (both themes) of the resolved badge +
  pills; update PR #41; delete this plan file, folding notes into the
  architecture docs.
