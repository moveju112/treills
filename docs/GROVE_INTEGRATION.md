# Grove integration contract (Treills API v1)

Treills is independent of Grove. The Rust collector reads local Claude/Codex transcripts and optional metadata hooks, normalizes them into SQLite, and exposes **read-only** HTTP JSON. The React UI is a reusable scoped component and does not read CLI files or SQLite directly.

## Scope and identity

- **Project**: canonical local Git common-dir path (worktrees share a project but keep their own path/branch); unrelated repositories are not combined by remote URL or name.
- **Session**: stable `s:<source>:<source-native-session-id>`; can have many executions/turns.
- **Execution / turn**: stable `t:<source>:<session-hash>:<source-native-turn-id>`; a `Workflow` response is the **view of this execution**, not an additional artificial workflow object. A session without identifiable turns is not given fabricated executions.
- **Agent node**: stable `<execution-id>:main` or `<execution-id>:agent:<source-agent-id>`. A connection requires an explicit parent agent or delegation identifier. Missing parent identifiers stay unlinked.
- **Child session**: `SessionRecord.parent_turn_id` references an established parent execution ID when the source supplies `parent_session_id` and `parent_turn_id`. Otherwise only native metadata is retained without inferred links.
- **Evidence**: event IDs are based on source event IDs when present, otherwise file identity/generation/byte offset. Neither prompt text nor tool arguments/outputs become evidence strings.

Session end means **ended**, not **success**. Recent file activity is **recent**, never proof of a running process. Test invocation is a tool event, not proof of passing. An explicit success/failure/cancelled outcome is required for those labels. Missing state evidence remains unknown.

## Read-only API

All JSON responses contain `api_version: "1.0"`.

| Endpoint | Query | Purpose |
| --- | --- | --- |
| `GET /api/v1/projects` | `filter=all\|active\|attention\|completed&search=&limit=30&offset=0` | Projects, counts and previews |
| `GET /api/v1/sessions` | `project_id=&source=&limit=30&offset=0` | Sessions within a project or across all projects |
| `GET /api/v1/sessions/:id/events` | `cursor=0&limit=100` | Session-scoped events, including unattributed events |
| `GET /api/v1/workflows` | `project_id=&session_id=&turn_id=&state=&search=&limit=30&offset=0` | Execution views, filtered **on the server** |
| `GET /api/v1/workflows/:id` | — | Execution, observed nodes/edges, first events |
| `GET /api/v1/workflows/:id/events` | `cursor=0&limit=100` | Execution event pagination |
| `GET /api/v1/changes` | `after=0&limit=100` | Durable change cursor to catch up after reconnect |
| `GET /api/v1/contract` | — | API version and polling interval |

The existing `monitor`, `sessions`, `events`, `doctor`, `install`, `uninstall`, and `run` commands are preserved. `serve` explicitly starts the web server; it binds to `127.0.0.1` by default with **no wildcard CORS**. Do not expose it publicly without adding authentication in the host application. SSH forwarding is the preferred remote-access method.

## Reusing the UI in Grove

Import `ActivityView` and its props from `web/src/index.ts` (or package it in Grove). The controlled component accepts:

```tsx
<ActivityView
  provider={groveProvider}
  scope={{ kind: 'session', id: selectedSessionId }}
  onScopeChange={setScope}
  selectedNodeId={selectedNodeId}
  onNodeSelect={setSelectedNodeId}
/>
```

`ActivityScope` kinds are `all`, `project`, `session`, `execution`. An embedded project tab can supply a project scope; a session panel can supply a session scope; a turn-specific detail can supply an execution scope. The standalone `Dashboard` provides a navigation wrapper around this same component.

Implement `ActivityProvider` in Grove. Replace `createHttpProvider` with an adapter to Grove's authorized project/session/turn/delegation APIs. Keep Grove's authentication, ACLs, project visibility and historical record ownership. Do not import the Treills SQLite store or require Treills login. The UI and graph take normalized data only, use scoped CSS Modules, and have no absolute API port in components or mandatory router/iframe integration.

Authoritative API model: `src/domain.rs`. Generate TypeScript declarations using `cargo run -- typegen`, then run `npm run typecheck` under `web`. The Rust `i64` timestamps and cursors are serialized as JSON numbers and explicitly mapped as TypeScript `number`.

## Poll and recovery

`ActivityView` polls `changes?after=<cursor>` every 2 seconds and consumes paginated responses until caught up. If a request fails, the old cursor is retained and the next request retries. Changes cause a scoped requery without replacing the selected execution or graph viewport.

The SQLite collector tracks each source's file identity, generation, prefix and committed byte offset; it continues from partial JSONL lines on a future scan. A replaced/truncated or corrupt file retains old history and creates an issue/incomplete marker. Source events without a trustworthy turn ID stay session-scoped, not copied into a made-up turn.

## Limits

Local CLI transcript schemas are unofficial and variable. There is currently no trustworthy OS-process liveness verification, no inference of missing cross-session parent edges, and no AI summaries of prompts. The app deliberately does not synthesize progress percentages or success results. UI components use the system light/dark theme, keyboard focus, reduced-motion CSS, and graph-local pan/zoom.
