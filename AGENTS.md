# AGENTS.md: Standing Instructions (WinView / Gander for Windows)

These rules bind any agent (or human) working in this repository.

## 0. Core rules (from project charter)

1. **Think before coding**: state assumptions; surface tradeoffs; present all interpretations; push back when a simpler approach exists.
2. **Simplicity first**: minimum viable code; no speculative abstractions; realistic error handling only; if 200 lines can be 50, rewrite.
3. **Surgical changes**: touch only what the task requires; match existing style; mention (don't delete) pre-existing dead code; every changed line traces to the task.
4. **Goal-driven execution**: every task has a verifiable check; loop until verified; never weaken tests to get green.

## 1. Canonical map

| Path | Role |
|---|---|
| `prd.md` | Requirements FR-#, goals G-#, non-goals, U-# |
| `spec.md` | Architecture, stack pins, contracts, security, traceability |
| `plan.md` | Phased tasks N.M with checkboxes + verification mapping; M-# matrix |
| `decisions.md` | Append-only ledger D-### (never edit old rows) |
| `research/` | Dated notes, sources, evidence tags DOC/UNDOC/KNOW/SPIKE |
| `src/` (future) | Frontend TS |
| `src-tauri/` (future) | Rust backend |

## 2. Hard safety rules

- **No networking code.** No fetch/http plugins, telemetry, analytics, update pings. Offline is a product invariant (G-4).
- **No GPL code.** QuickLook (QL-Win) is behavior-reference only (D-007).
- **Never write to previewed databases** (rusqlite immutable+read-only, D-013/spec §4).
- **Never open OneDrive cloud-only files** for read without the explicit user click (D-013).
- **Real `.env` values never leave Rust while hidden** (spec §5). Test this with a substring unit test.
- Ask before: pushing/tagging/releasing, installing anything system-wide, or modifying real system state (autostart, registry) outside a dedicated verification step.
- Never claim an OS version as supported unless its matrix rows were executed on it (D-005).

## 3. Workflow

1. One task (`N.M`) at a time, in `plan.md` order, after dependencies are checked.
2. Before coding: read `AGENTS.md`, the next unchecked task, and only the relevant `prd.md`/`spec.md`/`decisions.md` sections. State: goal, assumptions, file scope, verification check.
3. Implement the smallest surgical change + tests. Run targeted checks (`cargo test`, `cargo clippy`, `tsc --noEmit`).
4. Sync docs and append any `D-###` decisions in the same task.
5. Tick the checkbox only with evidence (command + result). Never skip checks.
6. End each task with a STATUS report (format below).
7. At phase end: full check suite, diff/secrets review, read-only reviewer subagent, then verification subagent.
8. Requirement changes flow: `prd.md` → `spec.md` → `plan.md` → `decisions.md` → code.

### STATUS report format (after each task)

```text
STATUS
- Done: <N.M and title> / <not complete, if blocked>
- Evidence: <commands, results, environment, files changed, commit if any>
- New tests needed: <none / tracked task and reason>
- Human verification: <exact steps, expected result, undo>
- Docs updated: <paths / none needed>
- Decisions: <D-### / none>
- UNRESOLVED / risks: <items / none>
- Next: <N.M and title>. Proceed? (y/n)
```

## 4. Verification checks (minimum bar)

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`: before ticking any Rust task.
- `tsc --noEmit`: before ticking any frontend task.
- Manual matrix rows referenced by the task executed on the dev machine (Win11 26200): record Result & Env in `plan.md` M-table.
- Markdown pipeline tasks: include a sanitization unit test proving `<script>` never survives.
- `.env` tasks: include the leakage substring test.
- SQLite tasks: include the "no new files in db directory" assertion harness (manual M-16).

## 5. Style

- Rust: 2021 edition, `rustfmt` defaults, no `unsafe` outside the `win32`/`shell` modules; every `unsafe` block gets a `// SAFETY:` comment.
- TS: strict mode, no framework, no build-time magic; plain modules + functions.
- Comments only where intent isn't obvious; match density of surrounding code.
