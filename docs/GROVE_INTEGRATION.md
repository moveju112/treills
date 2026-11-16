# Grove integration boundary (Treills API v1)

Treills is a standalone Rust collector + SQLite store + Axum query service. Its React UI is a separately exported component. Grove must supply its own authorized `ActivityProvider`; do **not** import the Treills SQLite implementation into Grove.

- Reuse: `web/src/Dashboard.tsx`, `Graph.tsx`, `AgentBox.tsx`, `graphLayout.ts`, `Dashboard.module.css`, `status.ts`, `provider.ts` interface and `contracts.generated.ts`.
- Replace: `createHttpProvider` with a Grove adapter that translates Grove projects, sessions, turns, delegation and verification events into the shared API contract. Keep Grove's existing authentication/authorization and project ACLs.
- Props: `provider`, `selectedProjectId`, `onProjectChange`, `selectedWorkflowId`, `onWorkflowChange`. Selection can be controlled by a host and remains stable during polling.
- Data: `GET /api/v1/projects`, `/api/v1/workflows`, `/api/v1/workflows/:id`, `/api/v1/workflows/:id/events?cursor=N`, `/api/v1/changes?after=N`. All responses include `api_version: "1.0"`; poll changes until `has_more=false`.
- Security: standalone `treills serve` defaults to `127.0.0.1:47821`, no CORS configuration or command-execution endpoints. For remote access use an SSH tunnel rather than public binding. Prompt bodies, tool argument bodies and outputs are not indexed or exposed by this application.
- CSS: Dashboard CSS Modules; standalone stylesheet is deliberately isolated in `standalone.tsx` and never imported by the reusable component entrypoint.
- Types: Rust source is `src/domain.rs`; run `cargo run -- typegen` then `npm run typecheck` and compare the checked-in generated types in CI.

## Known constraints

Original CLI transcript formats are unofficial and change over time. A transcript may lack task labels, parent agent identifiers, and terminal outcomes. Those remain unknown rather than synthesized. The collector persists append cursors, but a replaced transcript may carry previously seen content with new file offsets; the generation identity intentionally prevents false attribution, and ingestion issues mark incomplete histories. The UI must not equate recent activity with a living process or session end with success.
