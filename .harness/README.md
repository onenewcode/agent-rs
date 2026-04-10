# Harness Guide

This directory contains the shared harness that keeps Codex, Claude, and Gemini aligned on one policy and one task definition.

## What This Harness Does

- Generates agent-facing artifacts from:
  - `.harness/policy.toml` (policy source of truth)
  - `.harness/task.md` (task source of truth)
- Enforces command/path decisions through one Python hook engine.

## Quick Start

1. Edit policy or task:
   - `.harness/policy.toml`
   - `.harness/task.md`
2. Regenerate artifacts:
   - `python3 .harness/sync.py`
3. Verify outputs:
   - `.harness/instructions.md` (shared instruction document)
   - `.codex/hooks.json`
   - `.codex/rules/commands.md`
   - `.claude/settings.json`
   - `.gemini/settings.json`

## Optional Pointer Files (On-Demand Write)

`sync.py` only updates these files if they already exist:

- `AGENTS.md`
- `CLAUDE.md`
- `GEMINI.md`

When present, each file is reduced to a single pointer to `.harness/instructions.md`.
If these files do not exist, `sync.py` does not create them.

## How It Works

### 1) Sync Pipeline

`sync.py` reads policy + task and renders generated artifacts.

Generated instruction flow:

- `task.md` + policy summary -> `.harness/instructions.md`
- optional root pointer files -> link to `.harness/instructions.md`

Generated hook/config flow:

- Codex -> `.codex/hooks.json`
- Claude -> `.claude/settings.json`
- Gemini -> `.gemini/settings.json`

All of them call:

- `.harness/hooks/policy_hook.py`

### 2) Hook Decision Engine

`policy_hook.py` is the single runtime decision engine.

Core behavior:

- Parses command and splits compound commands (`&&`, `||`, `;`, `|`).
- Applies strictness precedence: `deny > warn > allow`.
- Enforces blocked command prefixes.
- Enforces blocked write paths.

### 3) Runtime Output

The harness does not persist runtime decision logs by default.
It only returns allow/warn/deny results through hook exit behavior.

## Policy Model (Current Defaults)

From `.harness/policy.toml`:

- enforcement scope: `writes_and_high_risk_only`
- unknown command mode: `allow_warn`
- path mode: `blocklist_only`
- blocked write path: `.git/**`

## Test

Run harness tests with:

- `python3 -m unittest discover -s .harness/tests -p 'test_*.py'`

## Design Constraints

- Python only
- No extra runtime CLI
- No sandbox implementation inside harness
- Keep behavior explicit and auditable
