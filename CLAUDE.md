# CLAUDE.md — project constitution (read fully before any task)

Context is thrown away every pass and re-derived from disk. Disk and git history
are the only memory. Re-read the relevant files at the start of work; write your
conclusions to disk, not just into your reply.

- No stubs, no simplified implementations, no fake passes. If you cannot
  implement the real thing, log a blocker and stop — a green that isn't real is
  the worst outcome in this system.
- Tests freeze at red-pass. Never edit a frozen file (tests, criteria_map, the
  detectors in .ratchet/detectors/) in a green phase.
- Judgment is deterministic: the compiler, the frozen tests, the frozen checker,
  and mutation decide — not your opinion.
- One task per worktree; tasks are small by design. If your context fills up,
  the task should have been split.
- Fix documents before code: reconcile plan.md, spec.md, list.md before building.
- Log all failures with raw tool output, not paraphrases.
- IDs are immutable. Splits add suffixes; nothing is renumbered or deleted.

Label any genuine placeholder per rule.md — but better, do not leave one.
