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

## Passing the mutation gate (re-read every RED and GREEN phase)

The mutation gate flips operators (`==`→`!=`, `>=`→`>`, `&&`→`||`, `+`→`-`, …) in
your implementation and requires the frozen tests to KILL every mutant. Two rules
follow, and ignoring either is the usual cause of a stalled task:

- **GREEN: write the MINIMAL implementation that satisfies the frozen tests.**
  Every operator and branch you write must be killed by a frozen test. If you add
  validation, precedence, bounds, or boundary logic the frozen tests do not
  exercise, a mutant will survive and the gate will reject the pass — and you
  cannot edit the frozen tests to fix it. When in doubt, write less; do exactly
  what the criteria require, nothing more.
- **RED: write tests that pin every branch and both sides of every boundary.**
  For each comparison or boolean the implementation will need, assert behaviour on
  both sides (e.g. at the threshold and just past it, true and false). A test that
  only checks the happy path leaves operator mutants alive and dooms the green
  phase. Cover every criterion with boundary-exercising assertions.
