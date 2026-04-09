# Agent Workflow

Use a plan-first workflow.

## Read Order

1. [PLANS.md](PLANS.md)
2. [docs/harness/EXECUTION_POLICY.md](docs/harness/EXECUTION_POLICY.md)
3. [docs/harness/GIT_GATES.md](docs/harness/GIT_GATES.md) before commit work
4. [docs/harness/README.md](docs/harness/README.md) only when you need the harness index

## Default Flow

1. Explore.
2. Create or update an ExecPlan for non-trivial work.
3. Wait for user confirmation before edits or high-impact mutations.
4. Implement.
5. Validate.
6. If repo changes exist, prepare a review package.
7. Wait for explicit human approval before `git commit`.
8. Update the active ExecPlan before finishing.

## Hard Rules

- Non-trivial work requires an ExecPlan.
- Do not change harness files unless the user asked for harness work.
- No destructive commands, history rewrites, or `git commit --no-verify`.
- No dependencies or networked shell commands without confirmation.
- Follow `docs/harness/EXECUTION_POLICY.md` for git mutation rules.
- No `git commit` until the active ExecPlan records human approval.
- Do not claim validation you did not run.

## Response Requirements

- Mention the active ExecPlan path before implementation work.
- Keep answers concise and factual.
- Before requesting commit approval, summarize changed files, validation, risks, and the proposed commit message.

## Traceability

- Harness changes go in `docs/harness/CHANGELOG.md`.
- Task progress and review state go in the active ExecPlan.
