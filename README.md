<h1 align="center">mend</h1>

<p align="center">
  <b>A single-shot AI coding assistant on the command line —<br>
  explain, edit, fix, and test your source files, one precise operation at a time.</b>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-2021%20edition-orange.svg" alt="Rust 2021">
  <a href="https://github.com/voldiguarddevelopment/Ratchet"><img src="https://img.shields.io/badge/built%20by-Ratchet%20%F0%9F%A6%8A-2bbac5.svg" alt="Built by Ratchet"></a>
</p>

---

> ### 🤖 This repository was built autonomously, test-first, by [**Ratchet**](https://github.com/voldiguarddevelopment/Ratchet).
>
> Every line of `mend`'s source was written by an AI agent driven through a hardened
> Ralph-Wiggum loop — one task at a time, **tests frozen before the implementation**,
> each task gated by the compiler, a stub-checker, the test suite, and **mutation
> testing**, then merged only when all of them passed. The proof is in the repo
> itself: read the commit history (`red (tests)` → `freeze tests` → `green (impl)`
> → `merge`) and the HMAC-chained audit log under `.ratchet/`. No green was ever
> faked — a claimed pass that didn't reconstruct from git + the frozen tests would
> have halted the run. `mend` is the proof of work.

---

## What is `mend`?

`mend` is a focused, **single-shot** AI coding assistant: you point it at one file
and ask for one thing. It is not a chat loop and not an editor plugin — it is a
sharp Unix tool that does exactly one operation and exits, so it composes cleanly
into scripts, pre-commit hooks, and pipelines.

```sh
mend explain src/auth.rs --symbol verify_token
mend edit    src/parser.rs "make the error messages include the line number" --apply
mend fix     src/cache.rs  --error "borrow checker: cannot borrow `self` twice"
mend test    src/money.rs
```

The interesting design choice — and the reason it's so testable — is that **the
LLM is the only non-deterministic part.** It sits behind a single trait
(`Completer`); *everything else* — gathering context from your file, assembling
the prompt, parsing the model's response, and applying the resulting edit — is
deterministic, pure Rust, and exhaustively unit-tested. Swap in a scripted mock
`Completer` and the entire pipeline becomes reproducible, byte for byte.

## Features

| Command | What it does |
| --- | --- |
| `mend explain <file> [--symbol <name>]` | Describe a file, or one function/type within it. Read-only. |
| `mend edit <file> "<instruction>"` | Apply a natural-language change to the file. |
| `mend fix <file> [--error <msg>]` | Repair a file given a compiler error or a failing test. |
| `mend test <file>` | Generate unit tests for the file's public API. |

Output controls, shared across the mutating commands:

- **`--dry-run`** (default) — print a coloured unified diff of the proposed change; touch nothing.
- **`--apply`** — write the change to disk.
- **`--json`** — emit a machine-readable result for tooling.

## How it works

A request flows through a small pipeline of deterministic stages, with the model
consulted exactly once in the middle:

```
  file ─▶ context ─▶ prompt ─▶ ┌──────────┐ ─▶ parse ─▶ patch ─▶ diff / apply
          gather     assemble  │ Completer │   response   engine
                               │  (LLM)    │
                               └──────────┘   ← the ONLY non-deterministic step
```

1. **Context** (`context`, `structure`, `imports`, `budget`) — read the target
   file, extract its structure (functions, types) and imports, and pack the most
   relevant slice into a token budget.
2. **Prompt** (`prompt`) — assemble the system + user messages deterministically
   from the command and the gathered context.
3. **Completer** (`mock` + a real backend) — the single LLM boundary, behind a
   trait. Tests use a scripted mock; no network is ever touched in CI.
4. **Parse** (`parsed_edit`, `fenced`, `search_replace`, `unified_diff`) — pull
   the change out of the model's reply, whether it came back as a fenced code
   block, a search/replace block, or a unified diff.
5. **Patch engine** (`match_exact`, fuzzy matching, conflict detection, apply) —
   locate where the edit belongs (exactly, or fuzzily when the file has drifted),
   detect conflicts, and apply it. This is the hardest, most-tested part: an edit
   that can't be placed unambiguously is reported as a conflict, never guessed.
6. **Output** — render a coloured unified diff (`--dry-run`), write it (`--apply`),
   or emit structured JSON (`--json`).

Because stages 1–2 and 4–6 are pure, a faked or weak implementation of any of them
is caught by the frozen tests and the mutation gate — which is precisely why
Ratchet could build this autonomously and *prove* it correct.

## Install

```sh
git clone https://github.com/voldiguarddevelopment/mend.git && cd mend
cargo build --release
install -m755 target/release/mend ~/.local/bin/mend   # or anywhere on PATH

mend --help
```

Requires a Rust toolchain (stable, 2021 edition).

## Configuration

`mend` reads `mend.toml` from the working directory (or the environment); every
field has a sane default:

```toml
model        = "claude-opus-4-8"   # or any model your endpoint serves
endpoint     = "…"                 # the LLM API base URL
token_budget = 8000                # max context tokens fed to the model per call
```

Each may also be set via `MEND_MODEL`, `MEND_ENDPOINT`, `MEND_TOKEN_BUDGET`
(env overrides the file, which overrides the defaults). The API token is read
from the environment and is **never** written to disk or logged.

## Why it's testable (and why that matters)

Most AI coding tools are hard to trust because their behaviour is entangled with a
non-deterministic model. `mend` inverts that: the model is a thin, mockable seam,
and the genuinely complex machinery — context selection, edit parsing across three
formats, and a fuzzy-matching patch engine with conflict detection — is ordinary,
deterministic Rust with a comprehensive test suite. Run it against the mock
`Completer` and you get the same output every time. That testability is what let an
autonomous agent build it under a regime where **a green that can't be faked** is
the only acceptable definition of "done."

## Built by Ratchet

[Ratchet](https://github.com/voldiguarddevelopment/Ratchet) is a TUI harness that drives
`claude` CLI agents to build software autonomously, test-first, with deterministic
gates that make a faked pass impossible. It built `mend` end to end:

- a plan of **32 tasks** was elicited from a one-paragraph brief and decomposed
  into `plan.md` / `spec.md` / `list.md`;
- each task ran in its own git worktree: **RED** wrote and froze failing tests
  from the task's acceptance criteria, then **GREEN** implemented against those
  frozen tests until the cascade — integrity → checker → compile → test →
  **mutation** — passed honestly;
- every attempt was recorded in an HMAC hash-chained journal that re-derives each
  "passed" from git + the frozen-test hashes, so the audit trail cannot lie.

You can verify it yourself: clone this repo and read the commit graph and
`.ratchet/journal/`. That's the whole point — the proof is reconstructable, not
asserted.

## License

[MIT](LICENSE).
