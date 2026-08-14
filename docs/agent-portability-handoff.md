# Agent portability handoff

Status: local coordination document. Do not include it in an upstream pull
request unless the maintainers ask for it.

Last checked against GitHub and `origin/main`: 2026-08-14.

This is the starting point for an agent with no prior conversation context. It
records the product goal, the architectural boundaries, the way the work should
be conducted, the honest state of the effort, and the next concrete actions.
It is not a frozen backlog. GitHub moves quickly, so verify every head and every
claim before acting.

## The first ten minutes

1. Read this document completely.
2. Read [NIP-AP](nips/NIP-AP.md) from `origin/main`, not from the current
   checkout. The current checkout contains unmerged experimental changes.
3. Read the agent feature rules in
   [`desktop/src/features/agents/AGENTS.md`](../desktop/src/features/agents/AGENTS.md),
   then compare that file with `origin/main` for the same reason.
4. Run `git fetch origin main` and record the new `origin/main` SHA.
5. Recheck the latest releases, recently merged agent work, open agent pull
   requests, and issue [#4301](https://github.com/block/buzz/issues/4301).
6. Do not develop in the repository root. It is a conflicted experimental
   branch and must be preserved as evidence.
7. Review active work at an exact commit. Do not review a branch name or a PR
   number in the abstract.
8. Work in a clean temporary worktree from the exact head being reviewed. Use
   the shared Cargo target where safe and remove the worktree when finished.
9. Before any public review or push, test the real production seam and perform
   an adversarial pass over authority, secrets, failure, concurrency, lifecycle,
   and user-facing claims.
10. Report only material movement. The user wants delivered outcomes, not a
    running narration of effort.

## The goal

An agent is portable when someone can move its reviewed definition,
instructions, Skills, and declared capability needs to another Project or
machine, create or select a local instance, and reconnect the required tools
without transferring credentials, private keys, hidden permissions, or
machine-specific launch configuration.

The receiving installation creates a fresh cryptographic identity. Moving the
original private key would turn portability into credential transfer. Identity
transfer would require a separate, explicit design with its own authorization
and review.

This work used to be called Agent Capsules. Stop using that name. It sounds
like a new product inside Buzz and makes a connected set of ordinary product
concepts look like a private project. Use the Buzz vocabulary:

- agent template
- agent
- Agent Snapshot
- Skill
- Tool
- Project Connection
- version
- Update agents

Use MCP only when discussing the technical protocol. Users connect Tools to a
Project; they should not have to understand the transport.

## Work from the user experience back

The complete journey should feel like one product, not a collection of storage
features.

1. A person creates or receives an agent template.
2. They review the exact instructions, Skill contents, and declared Tool needs.
3. Import creates a new local agent identity or lets them apply the template to
   an explicitly selected local agent.
4. Buzz shows which required Tools are missing in this Project.
5. The person binds each requirement to an approved Project Connection. Secret
   values remain on the execution target.
6. Buzz proves readiness before the agent can run.
7. Editing a linked agent makes the object choice explicit: edit this agent, or
   edit its template.
8. Publishing a template creates an immutable version. It does not silently
   change running agents.
9. Update agents shows the exact impact, lets the person select the affected
   agents, applies the version, proves readiness, and rolls back if replacement
   startup fails.

A technical implementation that forces a confusing or unsafe version of this
journey is not good enough merely because it is easier to build. Extra technical
work is justified when it produces a clearer, safer, and longer-lived product
boundary.

## Authority model

Every decision has one owner. Do not create a second registry or let a
convenient cache become authority.

| Object | Owns | Must not own |
| --- | --- | --- |
| Agent template version | Reviewed instructions, exact Skill content, model preferences, logical Tool requirements, stable content identity | Credentials, private keys, connection ids, commands, endpoints, target paths, permission grants |
| Project binding | The approved mapping from a logical requirement to a Project Connection | Portable definition content, credential values, runtime observations |
| Agent instance | Cryptographic identity, selected template version, private overrides, runtime policy | Shared template authority, other agents' state |
| Execution target | Installed runtimes, resolved commands, environment material, credential resolution, local policy | Portable definition authority |
| Runtime evidence | What this process and session actually received, exposed, and used | Desired configuration, authorization, portable claims |
| Launch document | Bounded, short-lived, target-specific execution input | A reusable definition or connection registry |

The sequence is requirement, binding, target resolution, launch handoff,
observed use. Do not collapse those states into one green Ready label.

`allowed-tools` in an Agent Skill is an untrusted request. It never becomes a
Buzz permission grant merely because the file was imported. Activation must
intersect the request with explicit Project bindings and target policy.

## What is actually on main

At the last check, `origin/main` was
`df9e773a13f17a270fd6531fc74948b8059d58c3`. The latest Desktop release was
v0.5.11. Recheck both before relying on this section.

Main has a useful beginning:

- [#1753](https://github.com/block/buzz/pull/1753) provides Agent Snapshot v1
  export and import. The receiving side mints a fresh identity.
- [#3278](https://github.com/block/buzz/pull/3278) adds visual and locked Agent
  Snapshot cards.
- [#5015](https://github.com/block/buzz/pull/5015) unifies the Add agent entry
  points for create, discover, and import.
- [#1968](https://github.com/block/buzz/pull/1968) makes the agent definition
  authoritative for prompt, model, and provider.
- [#4593](https://github.com/block/buzz/pull/4593) defines the private managed
  agent wire protocol, and [#5133](https://github.com/block/buzz/pull/5133)
  adds relay ingest for that state.
- [#4220](https://github.com/block/buzz/pull/4220) is now on main and hardens
  literal review of shared instructions. It merged after v0.5.11, so do not
  assume it is in that release.
- [#5681](https://github.com/block/buzz/pull/5681) enforces shared agent mention
  authorization at send boundaries.

Main does not yet provide the full portable system:

- Agent Snapshot v1 does not carry complete Agent Skills directories.
- Portable Skill content and stable Skill identity are not on main.
- Portable logical Tool requirements are not on main. The `tool_requirements`
  additions visible in this checkout's `NIP-AP` are local experimental work.
- Project Connections and per-agent bindings are not on main.
- The target does not yet resolve requirements into a canonical launch handoff
  through a complete product flow.
- Immutable template versions, impact-aware selection, readiness-gated rollout,
  and rollback are not on main.
- Private cross-device operating state is still active work, not a finished
  cross-device agent system.
- Template and instance editing remain unsettled in the user interface.

Do not describe the current product as a portable agent system. It can save and
load part of an agent definition. That is a real foundation, but it is not the
whole journey.

## The honest impact so far

No pull request authored by `wolfyy970` has merged into `block/buzz` as of this
handoff. Never imply otherwise.

There is still real collaborative movement:

- Issue [#4301](https://github.com/block/buzz/issues/4301) produced a detailed
  maintainer response from Brad Groux. He agreed with the three-boundary
  direction, corrected the identity claim, narrowed the ownership of adjacent
  work, and identified the unowned version, binding, selection, and rollout
  contract. That response is the best public statement of the current design.
- On [#5321](https://github.com/block/buzz/pull/5321), the author adopted the
  requested secret boundaries, size limits, redacted diagnostics, and encoded
  size validation. We approved exact head `a5e0a4c47c` after 42 tests, strict
  Clippy, formatting, and the escaping regression passed. It has not merged.
- On [#2957](https://github.com/block/buzz/pull/2957), Brad adopted narrower
  readiness and evidence language after our review. It has not merged.
- [#4164](https://github.com/block/buzz/pull/4164) received collaborator and
  public attention, but it became too large and was closed in favor of smaller
  pieces. [#5349](https://github.com/block/buzz/pull/5349) is the one-commit
  stdio launch-document extraction.
- We approved the first two PRs in the new definition stack, #5842 and #5843,
  at their exact heads. Those approvals are recorded on GitHub. This is review
  impact, not accepted code from us.
- #4220 merged, but there is no GitHub review or commit from us on that PR. Do
  not claim it as our contribution.

Measure progress by merged code, maintainer decisions, adopted changes, and
dependencies that actually converge. Comment count, local code volume, and
social attention are not delivery.

## Current GitHub position

Start from a fresh scan, not this list. The list records why the work mattered
at the handoff point.

### Highest priority: the active definition stack

Maxwellimus opened a four-part stack that moves agent definitions into shared
SDK and CLI surfaces. It is closer to maintainer-owned momentum than our old
vertical slice.

| PR | Exact head on 2026-08-14 | State and next action |
| --- | --- | --- |
| [#5842](https://github.com/block/buzz/pull/5842) shared NIP-AP shapes | `1341b815ac4d` | Approved by `wolfyy970`. Mergeable. Its red smoke job was unrelated. Do not comment again without a new head or question. |
| [#5843](https://github.com/block/buzz/pull/5843) shared definition text validation | `063ae69b4c7b` | Approved by `wolfyy970`. The SDK and Desktop contract tests passed, including the full Unicode scalar comparison. Its red Desktop job was an unrelated randomized passphrase test. |
| [#5844](https://github.com/block/buzz/pull/5844) CLI persona publication | `976da4f74e21` | Review next. It is green and mergeable, but large enough to require a full adversarial pass through the real CLI, snapshot, media, replacement, deletion, and privacy seams. |
| [#5845](https://github.com/block/buzz/pull/5845) CLI team publication | `99a2f1f6f75c` | Review only after #5844. Check member resolution, ambiguity, exact team ids, absent versus explicitly empty fields, replacement and tombstone ordering, and live relay behavior. |

The value of this stack is not that a CLI is inherently portable. It gives
Desktop and automation one definition wire contract. That makes later Skill
and requirement fields less likely to split into incompatible publishers.

### Our small open PRs

- [#5349](https://github.com/block/buzz/pull/5349), exact head `13994181ef5c`,
  is the best small dependency we own. It defines the bounded, versioned stdio
  launch document that #5321 can construct after landing. It is mergeable and
  awaiting human review. Do not add more transport or runtime behavior to it.
- [#4600](https://github.com/block/buzz/pull/4600), exact head `11e111bfc99e`,
  contains the reusable portable Skill content and digest work. The conceptual
  boundary is good, but it has no maintainer response and may need to be
  reconciled with the new shared SDK stack before it is worth touching again.
- [#5271](https://github.com/block/buzz/pull/5271), exact head `7ccc157ff6d2`,
  makes v1 snapshot imports fail closed on unknown fields. It is small and
  safety-relevant, but has no response. Recheck main for equivalent work before
  spending time on it.
- [#5463](https://github.com/block/buzz/pull/5463), exact head `8cc8d8fc2482`,
  accepts ordinary snapshot filenames. It is small, green, mergeable, and
  unreviewed. Recheck whether later Add agent changes supersede it.
- [#5325](https://github.com/block/buzz/pull/5325), exact head `17a480daff08`,
  is mergeable permission behavior, but it is not central to portability and
  has been idle. Do not let it displace moving definition work.

### Do not resume wholesale

- [#4588](https://github.com/block/buzz/pull/4588) Project connections is useful
  design and test archaeology, not a branch to drag across main again.
- [#5278](https://github.com/block/buzz/pull/5278) is conflicting. It was a
  targeted response to #4999 and should move only if that maintainer-owned work
  moves and still needs the fix.
- The old vertical slice and its large branches contain valuable implementation
  details, tests, failure cases, and UI decisions. Mine them for small changes.
  Do not propose or rebase the whole system.

## How to be strategic

Strategy here means creating an outcome over several turns in a changing
community. It is not publishing a vision and waiting for everyone else to adopt
it.

Hold a clear view of the best user experience and architecture. Hold it
lightly. When another contributor has a better answer, acknowledge it, explain
why it is better, and build on it. When an idea blurs authority, leaks secrets,
or creates a poor user experience, push back with specific evidence.

Use the step-back protocol before every substantial move:

1. Inspect the immediate PR, its exact code, and the user task it claims to
   solve.
2. Step back to current main, recent merges, active maintainers, and competing
   implementations. Ask whether the seam has moved.
3. Step back again to the wider agent world. Ask whether the proposed boundary
   aligns with sound external standards or whether Buzz has a good reason to
   challenge them.
4. Return to the smallest change that improves the user journey and can
   plausibly merge now.

Prioritize work in this order:

1. A maintainer or trusted contributor is already moving the relevant seam.
2. A small change unblocks that work or gives it a shared contract.
3. The change has an identifiable reviewer and a clear reason to merge.
4. The production behavior and tests fit in a reviewable diff.
5. Only then consider a new independent PR.

Do not treat every open PR equally. Moving maintainer-owned work comes before
an elegant branch in the wilderness.

## Public GitHub behavior

Use the natural-writer skill for every GitHub post.

Write in first person. Be concise, direct, and technically specific. Explain
what is needed, why it matters to the user or architecture, and how it helps the
current effort. One or two sentences of why are usually enough.

Post when one of these is true:

- a new exact head addresses prior feedback and needs re-review
- a maintainer asks a question
- someone adopts an idea or offers useful work and should be acknowledged
- a concrete ownership decision will unblock two or more active changes
- a tested fix is ready to cherry-pick
- a review found a real correctness, security, authority, or product blocker

Do not post for visibility alone. Do not repeat a review request without a new
head, new evidence, or a maintainer question. One clear ask is enough. If a
thread has several unanswered comments, stop. Let others respond.

When someone implements a requested change, acknowledge it and clear or update
the review at the exact commit. Collaboration includes saying when their idea
improved ours. It also includes refusing a change that still produces the wrong
result.

Do not write an essay into a busy PR. Put the most important finding first,
name the production consequence, identify the smallest good fix, and stop.

Never call overlap impact unless there is evidence. Distinguish:

- our commit merged
- our review caused a change
- a maintainer explicitly depends on our work
- independent work reached a similar design
- public attention with no code or decision consequence

## Code and review protocol

The recurring failure mode has been coding or commenting too early, then
finding edge cases and issuing repairs in public. Reverse that order.

### Before editing

Write down:

- the object being operated on
- the user action
- the source of authority for every changed field
- the desired state and the applied state
- the point where external side effects become irreversible
- the secret and privacy boundary
- the failure, cancellation, retry, and recovery states
- the concurrency and cross-device races
- the exact user-facing claim that must remain true

If those answers are unclear, the code is not ready to write.

### Test from risk, not from helpers

Every useful unit of work needs an adversarial review before it is pushed or
posted. When subagents are available and authorized, assign one to attack the
exact diff. Otherwise perform the same pass explicitly yourself.

Tests should cover the production seam. Private helper tests are useful but do
not prove that the command, runtime, persistence, and UI wiring agree.

For stateful agent work, consider all of these classes:

- valid and invalid input boundaries
- unknown fields and newer versions
- empty, missing, explicit clear, and inherited values
- secret redaction in errors, observers, logs, debug output, and serialization
- target and workspace changes during asynchronous work
- concurrent edit, delete, launch, stop, hydration, and retry
- failure before an external side effect
- failure after an external side effect but before local persistence
- cancellation during a partial write
- restart and crash recovery
- stale desired state versus the running generation
- symlinks, path traversal, file limits, expansion limits, and identity
  collisions when files are involved
- actual runtime observation, not configuration self-attestation
- copy that states only what the evidence proves

Run focused tests first, then the relevant crate and UI suites, formatting,
strict Clippy, type checking, diff checks, and `just ci` when the risk warrants
it. Desktop is a separate Cargo workspace. A root Cargo test does not test it.

Record the exact head and exact commands. If the head changes, the review is
stale. Recheck before posting.

## External direction to keep watching

These are inputs, not authorities. Recheck them periodically.

- The [Agent Skills specification](https://agentskills.io/specification) treats
  a Skill as a directory with `SKILL.md` plus arbitrary supporting files. Buzz
  should preserve exact reviewed contents rather than reduce a Skill to a name
  or one Markdown string. Its experimental `allowed-tools` field remains a
  request, not a grant.
- [eve](https://github.com/vercel/eve) is a filesystem-first agent framework
  with instructions, Skills, Tools, Connections, schedules, and deployment
  targets. Its separation between authored agent material and deployment
  credentials supports the same broad direction, though Buzz still needs its
  own Project, identity, review, and rollout model.
- The [MCP 2026-07-28 specification](https://blog.modelcontextprotocol.io/posts/2026-07-28/)
  moves the protocol toward stateless requests, stronger authorization, and an
  extension model. Do not freeze Buzz around old HTTP session assumptions.
- [ACP](https://agentclientprotocol.com/get-started/architecture) standardizes
  the client-to-agent runtime conversation and MCP handoff. It is not the
  portable definition, Project binding, or template version.

Adopt a wider standard when it produces a better user and interoperability
result. Challenge it when it imports hidden authority or weakens review.

## Local workspace safety

The repository root is currently on `feat/agent-capsule-vertical-slice` at
`9ef95509d1b8`. It contains many local commits and unresolved merge conflicts.
It is not a clean build target and it does not represent `origin/main`.

The coherent committed state through that head is archived in the fork branch
`codex/archive-agent-portability-vertical-slice-20260814`. The unfinished
cherry-pick and its conflicted index remain local and are not part of that
archive. They are not an implementation plan.

Do not reset, clean, rebase, resolve, or delete this work without explicit user
approval. Do not infer product state from files in this checkout. Use
`git show origin/main:<path>` or a clean worktree when evaluating main.

There are many persistent worktrees under `buzz-worktrees`, `.worktrees`, and
`buzz-agent-snapshot-v2-import`. They previously consumed hundreds of gigabytes.
Do not create another persistent worktree for a short review. Use `mktemp`,
remove it on exit, and reuse the shared Cargo target only when concurrent builds
will not corrupt or block it.

The useful PR worktrees at this handoff include:

- `buzz-worktrees/acp-mcp-redaction` for #5349
- `.worktrees/portable-agent-skills` for #4600
- `buzz-agent-snapshot-v2-import` and
  `buzz-worktrees/agent-snapshot-strict-v1` for #5271
- `buzz-worktrees/fix-pr5323-default-mode` for #5325
- `buzz-worktrees/fix-pr5321-secret-boundary` for #5463, despite the stale
  directory name

Resolve the branch and PR from Git before using any worktree. Directory names
have drifted.

When launching Buzz Desktop for testing, keep it on macOS Desktop 2. The user
works on Desktop 1. Avoid repeated relaunches and repeated keychain prompts.
Restart when the test requires it, but do not let automation take over the
human's screen.

The user's private Buzz server is separate from development. Inventory running
containers, volumes, images, ports, and restart policy before changing it. Do
not delete or repoint the canonical private instance because a development
container looks similar.

## Automation

The task has an active heartbeat automation named
`move-buzz-portability-work-forward`, scheduled every two hours. Each run must
start from current GitHub state, act only on material movement, use natural
GitHub prose, favor small mergeable work, and report what it changed and why.

Do not turn the automation into a stale PR watchlist. If nothing moved, stay
quiet. If the goal changes or the automation begins disrupting the current
conversation, update or delete it with the automation tool.

## How to work with KC

KC wants an autonomous collaborator, not an agreeable narrator.

- Lead with the outcome and evidence.
- Do not repeat his instructions back to him.
- Do not agree automatically. Show why the decision is sound or challenge it.
- Keep status reports honest about merged code, replies, reviews, and
  independent overlap.
- Explain the why, but do not sell.
- Keep public writing short enough for a maintainer with hundreds of PRs.
- Track elapsed time. Work in bounded increments and stop a loop that is not
  producing a mergeable result.
- If you made a mistake, say what was wrong and fix it. Do not defend the
  activity.
- Take the best shot after proper review. Failure is acceptable. Careless
  repetition is not.

The desired public voice is firm enough to move a decision and restrained
enough that people will keep working with us.

## Next actions

1. Fetch GitHub and verify the four exact heads in the #5842 to #5845 stack.
2. Confirm the #5842 and #5843 approvals still apply to their current commits.
3. Review #5844 from a clean worktree. Inspect the full CLI production path,
   not only its builders. Test publication, replace conflict, snapshot import,
   avatar boundaries, deletion confirmation, relay rejection, secret absence,
   and failure after media upload. Approve or request changes once.
4. Review #5845 only after #5844 is stable. Exercise team membership ambiguity,
   missing definitions, Desktop export compatibility, explicit empty fields,
   same-second replacement, and tombstone ordering against a relay.
5. Reassess whether #5842 to #5845 creates the shared seam #4600 should use.
   Ask a maintainer before restacking #4600 again.
6. Keep #5349 narrow and ready. If #5321 or a maintainer moves, recheck its
   exact head and answer promptly.
7. After any merge, step back and rebuild the map. The right next increment may
   change.

The target is not to preserve our design. The target is to make Buzz's agent
portability coherent, safe, and real. Our design is the best current proposal
until the code or another contributor proves a better one.
