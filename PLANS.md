# ExecPlan Policy

Use living execution plans for non-trivial work.

## When a Plan Is Required

Create or update an ExecPlan for any task that:

- changes behavior or public interfaces
- touches multiple files
- adds or updates dependencies
- changes build, test, or release flow
- changes repository governance or harness assets

Small read-only investigations and tiny isolated edits can stay plan-free.

## File Layout

- Active plans live in `docs/exec-plans/active/`
- Completed plans are moved to `docs/exec-plans/completed/`
- `docs/exec-plans/active/TEMPLATE.md` is the starting point

## Plan Requirements

Each ExecPlan must be self-contained and include:

- objective
- current context
- decisions already made
- implementation steps
- validation plan
- review gate status
- progress log
- remaining risks or follow-ups

Use the template fields in `docs/exec-plans/active/TEMPLATE.md` rather than inventing ad hoc sections.

## Workflow

1. Create or update the plan before implementation.
2. Reference the active plan path in implementation messages.
3. Update progress and validation notes during the task.
4. If the task produced repo changes, fill in the template's Review Gate fields before closing the task.
5. Move the file to `completed/` when the work is done and any approved commit has been created, or leave it in `active/` with explicit next steps.
