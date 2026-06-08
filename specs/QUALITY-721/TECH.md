# TECH: Child agents can access inherited plan documents
## Context
This change makes child agents launched from a plan-backed orchestration able to read the same planning document as the lead agent.
There is no sibling `PRODUCT.md` in this checkout. The behavior captured here comes from the QUALITY-721 investigation and the agreed design constraints:
- Hydrate the child with the same plan document identity.
- Make synced inherited plans editable using existing `edit_plans` semantics.
- Make unsynced inherited copies read-only.
- Leave conversation fork semantics out of scope.
### Current child-agent launch flow
The current `run_agents` implementation composes a prompt and delegates each child to `StartAgentExecutor::dispatch`; no plan document payload is included in the child launch.
- `app/src/ai/blocklist/action_model/execute/run_agents.rs:124-219 @ b24fce3` is the fan-out point.
- `app/src/ai/blocklist/action_model/execute/run_agents.rs:321-338 @ b24fce3` shows that `plan_id` currently only resolves approved orchestration config fields.
`StartAgentRequest` is the client-side boundary that carries a child launch from the action executor into pane/agent startup. It currently carries name, prompt, execution mode, parent conversation ID, and parent run ID, but no inherited document data.
- `app/src/ai/blocklist/action_model/execute/start_agent.rs:45-68 @ b24fce3` defines the request.
- `app/src/ai/blocklist/action_model/execute/start_agent.rs:414-493 @ b24fce3` emits it from both normal and prevalidated dispatch paths.
Local Oz child launches create a hidden child conversation and immediately send the child prompt in the same app process.
- `app/src/pane_group/pane/terminal_pane.rs:1611-1725 @ b24fce3` creates the local child task and hidden pane, then calls `send_agent_query_in_conversation`.
Remote child launches create a child conversation locally but start execution through the public agent run API.
- `app/src/pane_group/pane/terminal_pane.rs:1957-2095 @ b24fce3` builds `SpawnAgentRequest`, including prompt, config, parent run ID, and runtime metadata.
### Plan document model
Planning documents are already represented as `AIDocument`s. An `AIDocument` has a stable document ID, optional `sync_id`, title, version, editor, user edit status, and owning conversation.
- `app/src/ai/document/ai_document_model.rs:95-184 @ b24fce3` defines this state.
The model can create a local document, create a document from an existing Warp Drive notebook, restore a document from transcript state, and update the backing notebook when a synced document changes.
- `app/src/ai/document/ai_document_model.rs:276-427 @ b24fce3` covers creation and restore entrypoints.
- `app/src/ai/document/ai_document_model.rs:867-929 @ b24fce3` covers edit/version restoration.
- `app/src/ai/document/ai_document_model.rs:1038-1052 @ b24fce3` updates a backing notebook when a synced document changes.
`read_plans` and `edit_plans` are local document tools today.
- `app/src/ai/blocklist/action_model/execute/read_documents.rs:28-56 @ b24fce3` reads from `AIDocumentModel` and silently drops missing document IDs. This is why a remote child that only receives a parent document ID can see an empty result.
- `app/src/ai/blocklist/action_model/execute/edit_documents.rs:43-127 @ b24fce3` validates all search/replace diffs against current local document content and creates a new local document version without explicit optimistic-revision checks. This behavior should remain unchanged for editable documents.
### Server-side plan artifact prior art
There is server-side prior art for synced plan artifacts, but it is artifact-oriented rather than child-startup-oriented.
`GET /api/v1/agent/artifacts/:uid` reads a plan artifact by artifact UUID, authorizes via `ConversationArtifactID`, and returns notebook-backed plan markdown when `notebook_uid` exists.
- `../warp-server-3/router/handlers/public_api/agent_artifacts.go:17-75 @ 7656677` is the authenticated artifact route.
- `../warp-server-3/router/handlers/public_api/agent_artifacts.go:80-137 @ 7656677` reads the backing notebook for plan artifacts.
Conversation forking copies plan artifact rows by `AIDocumentID`, but the broader fork semantics remain independent and are not changed here.
- `../warp-server-3/logic/ai_conversation_fork.go:30-113 @ 7656677` materializes independent conversation forks and copies artifact rows.
- `../warp-server-3/logic/ai/multi_agent/artifacts/fork.go:30-206 @ 7656677` matches and converts copied artifacts, including plans.
### Cloud child task threading
Remote children cross a transport boundary. The local client sends `SpawnAgentRequest` to `POST /agent/run`, and the server persists task prompt/config metadata before workers start the child run.
- `app/src/server/server_api/ai.rs:199-256 @ b24fce3` defines `SpawnAgentRequest`.
- `../warp-server-3/router/handlers/public_api/agent_webhooks.go:268-335 @ 7656677` defines `RunAgentRequest`.
- `../warp-server-3/router/handlers/public_api/agent_webhooks.go:457-571 @ 7656677` converts it into `NewTaskParams`.
Local CLI/Oz task creation uses GraphQL `CreateAgentTaskInput`, which currently only accepts prompt, environment, parent run ID, and config JSON.
- `../warp-server-3/graphql/v2/mutations/create_agent_task.graphqls:4-29 @ 7656677` is the current input.
Cloud run execution has two persisted layers:
- `TaskDefinition` is stored on `ai_tasks` and represents the task's durable prompt/input.
- `AIRunExecutionInput` is stored on each `ai_run_executions` row and is the execution-scoped input used by workers and prompt resolution.
Relevant code:
- `../warp-server-3/model/types/ai_tasks.go:393-415 @ 7656677` defines `TaskDefinition`.
- `../warp-server-3/model/types/ai_run_executions.go:52-89 @ 7656677` defines `AIRunExecutionInput` and the conversion from task definition to execution input.
- `../warp-server-3/logic/ai/ambient_agents/execution.go:71-99 @ 7656677` queues hosted executions from `NewAIRunExecutionInputFromTaskDefinition(task.Definition)`.
Follow-up and handoff executions are the important threading edge case:
- Active queued/in-progress follow-ups append to both task prompt and active execution input prompt, preserving other execution input fields.
- Cloud-to-cloud handoff creates a fresh execution input with only prompt and query mode today, which would drop inherited plan fields unless the new fields are copied explicitly.
Relevant code:
- `../warp-server-3/logic/ai/ambient_agents/dispatcher.go:632-654 @ 7656677` appends follow-ups to task prompt and active execution input.
- `../warp-server-3/logic/ai/ambient_agents/dispatcher.go:683-708 @ 7656677` creates fresh handoff execution input.
The Oz driver starts the first turn through `AIAgentInput::StartFromAmbientRunPrompt`, passing only the ambient run ID and local context.
- `app/src/ai/agent_sdk/driver.rs:2882-2905 @ b24fce3` dispatches that input after driver setup.
The server side resolves the prompt from task/execution input for third-party harnesses, while Oz uses the same run/execution input through the multi-agent request path.
- `../warp-server-3/router/handlers/public_api/harness_support.go:74-121 @ 7656677` shows prompt resolution using `AIRunExecutionInput` when present.
## Proposed changes
Add an inherited-plan document payload that follows child-agent launch boundaries and hydrates a real `AIDocument` in the child conversation before the child receives its first prompt.
### Inherited plan payload
Define a client-side `InheritedPlanDocument` payload near the `StartAgentRequest` boundary.
It should contain:
- Stable `AIDocumentId`.
- Title.
- Markdown content.
- Current `AIDocumentVersion`.
- Optional server-backed sync identity, only when the source document is already saved with a server `SyncId`.
- Write policy.
The write policy should be:
- `EditableSynced` when the source document has a server sync ID. The child restores the document with that sync identity and uses existing `edit_plans` behavior. Edits create a new document version and call the existing notebook update path.
- `ReadOnlyUnsynced` when the source document is unsynced or only has an in-flight client sync ID. The child hydrates enough local state for `read_plans` to work, but `edit_plans` returns a clear read-only error for that inherited document. This does not attempt to sync an unsynced parent plan during child launch.
### Client launch flow
Resolve inherited plans in `RunAgentsExecutor` after `prepare_request_for_execution` normalizes approved orchestration config and before `dispatch_prepared_run_agents` starts per-child dispatches.
Resolution rules:
- The source is `request.plan_id`.
- When `plan_id` is empty, there is no inherited plan payload.
- When `plan_id` is non-empty, read the current document from `AIDocumentModel` using the plan ID.
- Construct exactly one payload snapshot and reuse it for every child in the batch.
This keeps all children in a batch on the same plan content and write policy.
Extend `StartAgentRequest` and `StartAgentExecutor::dispatch` to carry `Vec<InheritedPlanDocument>`.
Normal `start_agent` calls that are not plan-backed pass an empty list. The payload belongs at this boundary because it is the last common point before local Oz, local harness, and remote child startup diverge.
### Local Oz child hydration
Hydrate inherited plans for local Oz child conversations in `launch_local_no_harness_child` immediately after creating the hidden child conversation and before calling `send_agent_query_in_conversation`.
Add an `AIDocumentModel` helper such as `restore_inherited_document` that can restore with:
- specific document ID;
- optional sync ID;
- content;
- version;
- owning child conversation ID;
- write policy.
The helper should reuse existing editor/model setup paths where possible rather than duplicating editor creation logic.
### Remote Oz child task threading
For remote Oz child runs, add inherited plan documents to:
- `SpawnAgentRequest` in the client;
- `RunAgentRequest` in warp-server;
- `TaskDefinition`;
- `AIRunExecutionInput`.
The payload is task input context, not execution configuration, so it should not live in `AgentConfigSnapshot`.
`enqueueAgentRun` should put inherited plans on `NewTaskParams.Definition`, and `NewAIRunExecutionInputFromTaskDefinition` should copy them onto the first hosted execution input. This gives both the durable task row and the concrete execution row the same inherited-plan context.
Preserve inherited plan payloads across cloud child task threading:
- Active follow-up prompt appends should leave inherited plan fields untouched.
- Cloud-to-cloud handoff execution creation should use a helper that starts from the task definition's inherited plans and overrides only prompt/query mode, rather than constructing an `AIRunExecutionInput` that contains only the follow-up message.
- This keeps inherited plans available if an idled or ended cloud child wakes up in a fresh execution.
When the remote Oz driver starts from a task ID, it already fetches task metadata before executing. Extend the fetched task metadata or add a focused task-input fetch so the driver can access inherited plan documents. Hydrate them into `AIDocumentModel` before dispatching `StartFromAmbientRunPrompt`.
Third-party harnesses can ignore the payload because they do not use Warp's document tools.
### Local task metadata
For local child tasks created through GraphQL `createAgentTask`, add the same inherited plan field to `CreateAgentTaskInput` only if the task row needs the metadata for consistency or observability.
The local same-process Oz path should not depend on a server round trip for hydration; it can hydrate directly from `StartAgentRequest`.
### Document tool behavior
Update `AIDocumentModel` to track inherited-document write policy. Keep this scoped to inherited documents; do not introduce global stale-revision enforcement.
`ReadDocumentsExecutor` should continue reading hydrated documents normally.
`EditDocumentsExecutor` should check the write policy before validating diffs and return a clear error when a child tries to edit a read-only inherited plan. For editable synced inherited plans, preserve the existing all-or-nothing fuzzy-diff application and notebook sync behavior.
Make missing-document reads explicit. `ReadDocumentsExecutor` should return an error when any requested plan ID is absent from the current `AIDocumentModel` rather than silently returning `Success { documents: [] }`. Once inherited-plan hydration lands, a missing inherited plan indicates a launch/hydration bug or an invalid plan ID, and the tool should tell the model that directly.
### Out of scope
Do not change conversation fork semantics in this spec. Existing fork artifact copy behavior remains prior art and a risk to keep in mind, but QUALITY-721 is scoped to child-agent orchestration launches.
## Testing and validation
Add Rust unit tests around `RunAgentsExecutor` to verify that:
- a non-empty `plan_id` resolves the source document once;
- the same inherited plan payload is attached to every child dispatch;
- non-plan-backed `run_agents` calls are unchanged.
Add `StartAgentExecutor` or pane-launch tests for local Oz children verifying that:
- inherited plans are hydrated into the child conversation before the first query is sent;
- `read_plans` in that child can return the hydrated content.
Add tests for inherited write policy in `AIDocumentModel` and `EditDocumentsExecutor`:
- synced inherited plans are editable with existing fuzzy-diff behavior;
- unsynced inherited plans return a read-only error;
- ordinary local plans keep current edit behavior.
Add a `ReadDocumentsExecutor` test for missing requested IDs so the tool returns a clear error instead of an empty success object.
Add serialization tests for the remote transport payload on `SpawnAgentRequest` and the corresponding server request/task storage type. The test should cover both editable synced and read-only unsynced inherited plans, and should verify that third-party harness config is unaffected.
Add server tests around `TaskDefinition` to `AIRunExecutionInput` conversion and cloud-to-cloud handoff execution creation so inherited plans survive initial execution creation and follow-up execution requeueing.
Add an agent-driver startup test that constructs task input metadata with inherited plans and verifies the driver hydrates `AIDocumentModel` before the first Oz prompt is dispatched.
Manual validation should cover:
- A local lead agent creating a plan, launching a remote Oz child with `run_agents.plan_id`, and the child successfully calling `read_plans` with that plan ID.
- Launching from an unsynced local plan and confirming the child can read but receives a read-only error if it tries to edit.
Run `cargo fmt` and targeted `cargo check` for the touched Rust crates. Server-side schema/type changes should run the repo's normal codegen path and focused Go tests for the changed public API/task metadata behavior.
## Parallelization
Parallel sub-agents are not proposed for the first implementation.
The changes cross a narrow set of tightly coupled boundaries:
- `RunAgentsExecutor` creates the payload.
- `StartAgentRequest` transports it.
- Local and remote child startup hydrate it.
- Document tools consume the resulting `AIDocumentModel` state.
Splitting these across agents would increase merge coordination cost because type shape changes and tests need to stay synchronized. The best implementation sequence is server/transport type additions first if needed, then client launch/hydration, then document tool behavior and tests.
If the implementation expands substantially, split ownership by code area but keep the work in the existing checkouts:
- Server transport/schema changes in `../warp-server-3`.
- Client launch, hydration, and document-tool changes in this repo (`warp-3`).
Integration and validation ownership should remain with the integrator to avoid inconsistent generated types.
## Risks and mitigations
Remote child hydration depends on task metadata being available before the first Oz prompt.
- Keep hydration in the driver startup path before `StartFromAmbientRunPrompt` dispatch.
- Add a regression test for this ordering.
Unsynced inherited plans are intentionally read-only.
- This avoids creating a second unsynced source of truth while still letting the child inspect the plan.
Existing `edit_plans` behavior remains fuzzy-match based and all-or-nothing per tool call.
- This avoids introducing new stale-revision policy in QUALITY-721.
- Concurrent edits can still fail with content mismatch when search text no longer matches.
The server artifact endpoint reads synced plan artifacts by artifact UUID, not by `AIDocumentId`.
- This spec does not rely on that endpoint for child startup.
- Inherited-plan hydration carries the document content explicitly through launch/task metadata.
