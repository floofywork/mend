### T-00.01  Scaffold Cargo binary crate
id: T-00.01
phase: 0
status: done
depends_on: []
stack: rust
criteria:
  - C1: `cargo build` on the scaffold exits 0 and emits a binary named `mend` under target/debug.
  - C2: Cargo.toml declares package name `mend` with edition 2021 or later.
  - C3: `cargo test` exits 0 on the unmodified scaffold with zero tests.
not_doing:
  - No CLI parsing, subcommand, or business logic.
  - No dependency wiring beyond what the empty crate needs to build.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The root surface every other task builds on. Inputs: none beyond a Cargo manifest; bounds: a buildable empty binary crate. Outputs: a compiling `mend` binary and a green empty test run. Errors/edges: a manifest that fails to parse is the only failure, surfaced by cargo itself. Invariant: the workspace always compiles from this point forward. Done-check: the three criteria observable via cargo. Kept deliberately inert so later modules attach without rework.

### T-00.02  Define error type
id: T-00.02
phase: 0
status: done
depends_on: []
stack: rust
criteria:
  - C1: a public enum `MendError` implements `std::error::Error` and `std::fmt::Display`.
  - C2: `MendError` has distinct variants for Io, Config, Parse, Patch, and Completer failures.
  - C3: each variant's `Display` renders a non-empty human-readable message.
  - C4: a public alias `type Result<T> = std::result::Result<T, MendError>` is exported.
not_doing:
  - No conversion impls for third-party error types beyond `std::io::Error`.
  - No per-subcommand error wrapping.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
A single error vocabulary so every deterministic layer surfaces failure uniformly rather than panicking. Inputs: none; this is a type definition. Outputs: the `MendError` enum and `Result` alias. Errors/edges: the type itself is the edge-handling mechanism for the whole system. Invariant: every fallible function in the project returns `Result<T>`. Done-check: the enum compiles, the trait impls resolve, and Display strings are non-empty. Scope fenced to the taxonomy; mapping concrete failures into variants happens in the owning tasks.

### T-00.03  Define CLI parser
id: T-00.03
phase: 0
status: done
depends_on: []
stack: rust
criteria:
  - C1: parsing `explain f.rs` yields an Explain command with path `f.rs` and symbol `None`.
  - C2: parsing `explain f.rs --symbol foo` sets the symbol to `Some("foo")`.
  - C3: parsing `edit f.rs "do x"` yields an Edit command holding path and instruction `"do x"`.
  - C4: `--apply` and `--json` parse to booleans; with neither present, dry-run defaults to true.
  - C5: an unknown subcommand exits non-zero and prints usage to stderr.
not_doing:
  - No execution of any command; this only produces the parsed command struct.
  - No config or Completer construction.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The boundary that turns argv into a typed command. Inputs: process arguments; bounds: the four subcommands with their flags. Outputs: an enum of parsed commands carrying validated paths, optional symbol, instruction, error message, and output-mode flags. Errors/edges: unknown subcommand, missing positional, and mutually exclusive flag handling surface as a non-zero exit with usage. Invariant: a successfully parsed command is structurally complete for its variant. Done-check: the five parsing assertions. Scope fenced to parsing so dispatch stays a later concern.

### T-01.01  Define Completer trait
id: T-01.01
phase: 1
status: done
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: a trait `Completer` exposes `fn complete(&self, prompt: &str) -> Result<String>`.
  - C2: `Box<dyn Completer>` compiles, proving the trait is object-safe.
  - C3: the prompt is borrowed (`&str`), not consumed, by the signature.
not_doing:
  - No real or mock implementation of the trait.
  - No streaming or multi-turn conversation surface.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The single seam that isolates non-determinism. Inputs: a borrowed prompt string; bounds: one synchronous call returning a completion or a `MendError::Completer`. Outputs: the trait definition only. Errors/edges: all backend failure collapses into the `Result` return. Invariant: every layer above depends on this trait, never on a concrete backend, so tests stay deterministic. Done-check: object-safety and signature compile checks. Fenced to the abstraction; implementations are separate tasks.

### T-01.02  Implement mock Completer
id: T-01.02
phase: 1
status: done
depends_on: [T-01.01]
stack: rust
criteria:
  - C1: `MockCompleter::new(responses)` returns its scripted responses in FIFO order across successive `complete` calls.
  - C2: calling `complete` more times than scripted returns `MendError::Completer`.
  - C3: the mock records every received prompt, exposed via an accessor for assertions.
  - C4: `MockCompleter` implements `Completer`.
not_doing:
  - No network access of any kind.
  - No response templating or prompt-conditional logic.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The deterministic stand-in that makes end-to-end tests possible without a network. Inputs: a pre-scripted vector of responses; bounds: one response consumed per call. Outputs: scripted completions plus a recorded prompt log. Errors/edges: exhausting the script is a deterministic `Completer` error, not a panic. Invariant: identical scripts and call sequences yield identical behaviour every run. Done-check: the FIFO, exhaustion, recording, and trait-impl criteria. Scope fenced strictly to test-driving behaviour.

### T-01.03  Load configuration
id: T-01.03
phase: 1
status: done
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: `Config::load` reads `model`, `endpoint`, and `token_budget` from `mend.toml` in the working directory.
  - C2: env vars `MEND_MODEL`, `MEND_ENDPOINT`, `MEND_TOKEN_BUDGET` override the corresponding file values.
  - C3: a missing `mend.toml` yields populated defaults without error.
  - C4: a non-integer `token_budget` (file or env) yields `MendError::Config`.
not_doing:
  - No remote or per-command configuration sources.
  - No secret/credential management.
test_files: []
criteria_map: {}
attempts: 4
last_failure: ""
---
The settings boundary feeding model, endpoint, and budget into the deterministic layers. Inputs: a TOML file and three env vars; bounds: three typed fields with a precedence rule. Outputs: a `Config` struct. Errors/edges: missing file falls back to defaults; malformed integer budget is a `Config` error. Invariant: env always overrides file, file always overrides defaults. Done-check: the four load/override/default/error criteria. Fenced so token budgeting itself lives elsewhere.

### T-02.01  Extract file structure
id: T-02.01
phase: 2
status: done
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: given Rust source, returns the signatures of every public `fn`, `struct`, `enum`, and `trait`.
  - C2: items not marked `pub` are excluded from the returned structure.
  - C3: an empty file yields an empty structure list, not an error.
  - C4: source that fails to parse yields `MendError::Parse`, never a panic.
not_doing:
  - No non-Rust languages.
  - No body extraction; signatures only.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The first half of context gathering: a structural summary of the target. Inputs: a Rust source string; bounds: top-level public item signatures. Outputs: an ordered list of signature strings. Errors/edges: empty input is valid and empty; unparseable input is a `Parse` error caught before it can panic. Invariant: only the public API surface is ever included. Done-check: the four extraction criteria. Fenced to one language and to signatures so the task stays small.

### T-02.02  Extract imports
id: T-02.02
phase: 2
status: done
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: returns every `use` path declared in the file as strings.
  - C2: a grouped `use a::{b, c}` expands to `a::b` and `a::c`.
  - C3: a file with no imports yields an empty list.
not_doing:
  - No transitive dependency resolution.
  - No glob (`*`) expansion into concrete names.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The second half of context gathering: the file's declared dependencies. Inputs: a Rust source string; bounds: `use` declarations only. Outputs: a flat list of fully-qualified import path strings. Errors/edges: no imports yields an empty list; grouped imports expand deterministically. Invariant: each returned string is a single concrete path. Done-check: the three criteria. Fenced away from resolution so it remains a pure syntactic pass.

### T-02.03  Budget context tokens
id: T-02.03
phase: 2
status: done
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: `budget(text, n)` returns text whose estimated token count is at most `n`.
  - C2: estimation uses a fixed deterministic chars-per-token ratio with no model or network call.
  - C3: when truncation occurs the result ends with a visible truncation marker.
  - C4: a budget of 0 returns an empty string.
not_doing:
  - No model-specific BPE tokenizer.
  - No semantic-aware truncation (line/char boundary only).
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The mechanism that keeps prompts within the configured budget deterministically. Inputs: a text string and an integer budget; bounds: budget ≥ 0. Outputs: text guaranteed within the estimated budget, marked if cut. Errors/edges: zero budget yields empty; oversize input truncates. Invariant: estimated tokens of the output never exceed `n`. Done-check: the four criteria. Fenced to a deterministic heuristic so tests never depend on a real tokenizer.

### T-02.04  Assemble file context
id: T-02.04
phase: 2
status: done
depends_on: [T-02.01, T-02.02, T-02.03, T-01.03]
stack: rust
criteria:
  - C1: combines extracted structure and imports into one deterministic context string for a file.
  - C2: the assembled context passes through the budgeter and never exceeds the configured token budget.
  - C3: identical inputs always produce byte-identical output.
not_doing:
  - No command-specific framing or instructions.
  - No Completer invocation.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The join point that produces the single context blob fed to prompt assembly. Inputs: a file's extracted structure, imports, and the configured budget; bounds: output ≤ budget. Outputs: one deterministic context string. Errors/edges: oversize context is truncated by the budgeter rather than erroring. Invariant: same file plus same config always yields the same bytes. Done-check: the determinism and budget-bound criteria. Fenced from prompt framing so the next task owns command-specific shaping.

### T-03.01  Assemble command prompts
id: T-03.01
phase: 3
status: done
depends_on: [T-02.04]
stack: rust
criteria:
  - C1: the explain prompt embeds the file context and, when present, the target symbol name.
  - C2: the edit prompt embeds the context and the user instruction verbatim.
  - C3: the fix prompt embeds the context and the supplied error message.
  - C4: the test prompt embeds the context and requests tests for the public API only.
  - C5: every prompt builder is a pure function of its inputs with no I/O.
not_doing:
  - No Completer invocation.
  - No response parsing.
test_files: []
criteria_map: {}
attempts: 1
last_failure: ""
---
The deterministic prompt layer, one builder per subcommand over a shared module. Inputs: the assembled context plus per-command parameters (symbol, instruction, error, none); bounds: each builder is total over its inputs. Outputs: four prompt strings. Errors/edges: absent optional inputs (symbol, error) are handled by omission, not error. Invariant: every builder is referentially transparent. Done-check: the five embedding/purity criteria. Fenced to construction so the LLM round-trip stays in the command tasks.

### T-04.01  Define parsed-edit model
id: T-04.01
phase: 4
status: pending
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: a `ParsedEdit` type represents either a (search_text, replace_text) pair or a whole-file body.
  - C2: a `Vec<ParsedEdit>` represents a multi-hunk response.
  - C3: `ParsedEdit` derives `PartialEq` and `Debug` for test assertions.
not_doing:
  - No parsing logic.
  - No patch application.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The shared currency all three parsers emit and the patch engine consumes. Inputs: none; a data definition. Outputs: the `ParsedEdit` type. Errors/edges: none at this layer; it is a value type. Invariant: every response format reduces to a `Vec<ParsedEdit>`. Done-check: the variant, collection, and derive criteria. Fenced to the model so parser and applier tasks share one contract.

### T-04.02  Parse fenced code blocks
id: T-04.02
phase: 4
status: pending
depends_on: [T-04.01]
stack: rust
criteria:
  - C1: extracts the body of a triple-backtick fenced block as a whole-file `ParsedEdit`.
  - C2: the language tag after the opening fence is stripped from the body.
  - C3: input with an unterminated fence yields `MendError::Parse`.
  - C4: multiple fenced blocks return one ParsedEdit per block in document order.
not_doing:
  - No search-replace or diff parsing.
  - No language validation of the tag.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The simplest response format: a whole-file replacement in a code fence. Inputs: an LLM response string; bounds: triple-backtick fences. Outputs: whole-file `ParsedEdit`s in order. Errors/edges: an unclosed fence is a `Parse` error. Invariant: each emitted edit carries the verbatim fenced body without the language tag. Done-check: the four criteria. Fenced to one format so detection logic stays separate.

### T-04.03  Parse search-replace blocks
id: T-04.03
phase: 4
status: pending
depends_on: [T-04.01]
stack: rust
criteria:
  - C1: parses a `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block into a (search, replace) ParsedEdit.
  - C2: multiple blocks yield multiple ParsedEdits in document order.
  - C3: a block missing the `=======` divider yields `MendError::Parse`.
not_doing:
  - No fuzzy matching or application.
  - No nested-block handling.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The targeted-edit response format. Inputs: an LLM response string; bounds: the three sentinel markers. Outputs: (search, replace) `ParsedEdit`s. Errors/edges: a malformed block missing its divider is a `Parse` error. Invariant: search and replace halves are split exactly on the divider. Done-check: the three criteria. Fenced from matching logic, which belongs to the patch engine.

### T-04.04  Parse unified diff
id: T-04.04
phase: 4
status: pending
depends_on: [T-04.01]
stack: rust
criteria:
  - C1: parses `@@ … @@` hunks of `+`/`-`/space lines into ParsedEdits.
  - C2: context (leading-space) and removed (`-`) lines populate the search side of each edit.
  - C3: a hunk header that fails to parse yields `MendError::Parse`.
not_doing:
  - No multi-file diff headers.
  - No patch application.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The third response format: a unified diff. Inputs: an LLM response string; bounds: single-file `@@` hunks. Outputs: `ParsedEdit`s whose search side reconstructs the pre-image. Errors/edges: a malformed hunk header is a `Parse` error. Invariant: each hunk maps to one edit whose search and replace sides are derived solely from its lines. Done-check: the three criteria. Fenced to single-file diffs to keep the parser bounded.

### T-04.05  Detect response format
id: T-04.05
phase: 4
status: pending
depends_on: [T-04.02, T-04.03, T-04.04]
stack: rust
criteria:
  - C1: a response containing `<<<<<<< SEARCH` routes to the search-replace parser.
  - C2: a response containing `@@ ` hunk markers routes to the unified-diff parser.
  - C3: a response with only a fenced block routes to the fenced parser.
  - C4: a response matching no known format yields `MendError::Parse`.
not_doing:
  - No patch application.
  - No confidence scoring beyond fixed first-match precedence.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The dispatcher that picks a parser from a raw response. Inputs: an LLM response string; bounds: the three known formats. Outputs: a `Vec<ParsedEdit>` from the selected parser. Errors/edges: no recognizable format is a `Parse` error. Invariant: format selection follows a fixed precedence so identical responses always route identically. Done-check: the four routing criteria. Fenced to selection so each parser stays single-purpose.

### T-05.01  Match exact context
id: T-05.01
phase: 5
status: pending
depends_on: [T-04.01]
stack: rust
criteria:
  - C1: locates the unique line range in the source equal to an edit's search text.
  - C2: search text absent from the source returns a no-match signal, never a panic.
  - C3: search text occurring more than once returns an ambiguity signal.
not_doing:
  - No whitespace-tolerant or fuzzy matching.
  - No mutation of the source.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The precise half of the matcher. Inputs: source text and an edit's search text; bounds: exact line-range equality. Outputs: a matched range, a no-match signal, or an ambiguity signal. Errors/edges: absence and multiplicity are signalled, not errored or panicked. Invariant: a returned range is byte-equal to the search text. Done-check: the unique-match, no-match, and ambiguity criteria. Fenced from fuzzy logic so each matcher is independently testable.

### T-05.02  Match fuzzy context
id: T-05.02
phase: 5
status: pending
depends_on: [T-05.01]
stack: rust
criteria:
  - C1: matches search text ignoring per-line leading/trailing whitespace differences.
  - C2: returns the matched range plus a similarity score in the range [0,1].
  - C3: candidate matches below a fixed similarity threshold are rejected as no-match.
not_doing:
  - No exact matching (delegates to T-05.01).
  - No token-level or semantic similarity.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The tolerant fallback when exact matching fails. Inputs: source text and search text; bounds: whitespace-insensitive line comparison with a fixed threshold. Outputs: a range and a similarity score, or no-match. Errors/edges: below-threshold candidates are rejected rather than forced. Invariant: any accepted match scores at or above the threshold. Done-check: the whitespace, scoring, and threshold criteria. Fenced so exact matching remains the first-choice path.

### T-05.03  Detect conflicts
id: T-05.03
phase: 5
status: pending
depends_on: [T-05.01, T-05.02]
stack: rust
criteria:
  - C1: flags two edits whose matched source ranges overlap as a conflict.
  - C2: flags an edit whose search text no longer matches after a prior edit applies as a conflict.
  - C3: a conflict-free edit set returns an empty conflict list.
not_doing:
  - No automatic conflict resolution.
  - No source mutation.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The validation pass that runs before any application. Inputs: source text and an ordered `Vec<ParsedEdit>`; bounds: overlap and post-application match checks. Outputs: a list of detected conflicts. Errors/edges: an empty list is the success signal. Invariant: an empty conflict list guarantees the edit set applies cleanly. Done-check: the overlap, stale-match, and clean-set criteria. Fenced from application so the applier can assume validated input.

### T-05.04  Apply edits
id: T-05.04
phase: 5
status: pending
depends_on: [T-05.03]
stack: rust
criteria:
  - C1: applies an ordered `Vec<ParsedEdit>` to source text and returns the new text.
  - C2: a whole-file ParsedEdit replaces the entire source.
  - C3: a conflicting or no-match edit yields `MendError::Patch` and leaves the source unchanged.
not_doing:
  - No disk writes.
  - No diff rendering.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The engine that produces new file content from validated edits. Inputs: source text and an ordered edit list; bounds: non-conflicting, matchable edits. Outputs: the rewritten text. Errors/edges: any conflict or no-match aborts with `Patch` and an unchanged source (transactional). Invariant: application is all-or-nothing. Done-check: the apply, whole-file, and abort criteria. Fenced to in-memory transformation so I/O stays in the output layer.

### T-06.01  Compute unified diff
id: T-06.01
phase: 6
status: pending
depends_on: [T-00.02]
stack: rust
criteria:
  - C1: given old and new text, returns a unified diff with `@@` hunk headers.
  - C2: unchanged regions are elided to a fixed context-line count.
  - C3: identical inputs yield an empty diff.
not_doing:
  - No colouring.
  - No file I/O.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The diff computation feeding both human and JSON output. Inputs: old and new text; bounds: single-file unified format. Outputs: a unified diff string. Errors/edges: identical inputs yield an empty diff (no spurious hunks). Invariant: applying the diff to old reproduces new. Done-check: the header, elision, and empty-diff criteria. Fenced from colour and I/O so it stays a pure function.

### T-06.02  Render coloured diff
id: T-06.02
phase: 6
status: pending
depends_on: [T-06.01]
stack: rust
criteria:
  - C1: added lines render green, removed lines red, and hunk headers cyan via ANSI codes.
  - C2: when stdout is not a TTY, all colour codes are omitted.
  - C3: an empty diff renders an empty string.
not_doing:
  - No diff computation (consumes T-06.01 output).
  - No paging or terminal-width wrapping.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The human-facing presentation of a diff. Inputs: a unified diff string and a TTY signal; bounds: per-line ANSI styling. Outputs: a coloured (or plain) diff string. Errors/edges: non-TTY output strips colour; empty diff stays empty. Invariant: stripping ANSI codes recovers the original diff text. Done-check: the colour, non-TTY, and empty criteria. Fenced from computation so rendering is purely presentational.

### T-06.03  Emit JSON result
id: T-06.03
phase: 6
status: pending
depends_on: [T-04.01]
stack: rust
criteria:
  - C1: serializes path, original text, proposed text, and the edit list to JSON.
  - C2: the JSON includes an `applied` boolean reflecting whether changes were written.
  - C3: the emitted JSON parses back into the same structure.
not_doing:
  - No colour or human-readable diff.
  - No partial/streaming output.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The machine-readable output format. Inputs: path, original and proposed text, edits, and an applied flag; bounds: one JSON object. Outputs: a serialized JSON string. Errors/edges: round-trip parse equality guards the schema. Invariant: the JSON fully reconstructs the result struct. Done-check: the serialize, applied-flag, and round-trip criteria. Fenced from the human path so the two output modes evolve independently.

### T-06.04  Dispatch output mode
id: T-06.04
phase: 6
status: pending
depends_on: [T-05.04, T-06.02, T-06.03]
stack: rust
criteria:
  - C1: in dry-run (default) mode the coloured diff prints and no file is modified.
  - C2: in `--apply` mode the new text is written to the target path on disk.
  - C3: in `--json` mode the JSON result prints and no diff is rendered.
  - C4: a patch failure under `--apply` writes nothing and returns `MendError::Patch`.
not_doing:
  - No command-specific prompt or parsing logic.
  - No backup/undo of overwritten files.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The single place that decides what reaches the user and the disk. Inputs: the applied result, original/proposed text, edits, and the output-mode flags; bounds: exactly one of three modes. Outputs: a printed diff, a written file, or printed JSON. Errors/edges: an apply-time patch failure is non-destructive. Invariant: only `--apply` ever touches disk. Done-check: the four mode criteria. Fenced so each subcommand reuses one output contract.

### T-07.01  Run explain command
id: T-07.01
phase: 7
status: pending
depends_on: [T-03.01, T-01.01, T-06.03]
stack: rust
criteria:
  - C1: gathers context, builds the explain prompt, calls the Completer, and prints the response text.
  - C2: with `--symbol` the prompt targets that symbol; absent, it targets the whole file.
  - C3: a Completer error exits non-zero with the message on stderr.
not_doing:
  - No patch application (explain never edits a file).
  - No diff rendering.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The read-only subcommand. Inputs: a target path, optional symbol, config, and a Completer; bounds: produces text, never edits. Outputs: the model's explanation printed to stdout (or JSON). Errors/edges: a Completer failure surfaces as a non-zero exit. Invariant: explain leaves the target file untouched. Done-check: the gather/print, symbol-targeting, and error criteria. Fenced from the patch path because explanation has no edits.

### T-07.02  Run edit command
id: T-07.02
phase: 7
status: pending
depends_on: [T-03.01, T-04.05, T-06.04, T-01.01]
stack: rust
criteria:
  - C1: gathers context, builds the edit prompt, parses the response, and routes edits to output dispatch.
  - C2: the user instruction passes through to the prompt unchanged.
  - C3: a parse failure surfaces as `MendError::Parse` with a non-zero exit.
not_doing:
  - No error-message handling (that is the fix command).
  - No test generation.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The instruction-driven mutation subcommand. Inputs: a path, an instruction, config, and a Completer; bounds: one round-trip producing edits. Outputs: edits routed through the output dispatcher. Errors/edges: an unparseable response is a `Parse` error and non-zero exit. Invariant: the instruction reaches the prompt verbatim. Done-check: the route, passthrough, and parse-failure criteria. Fenced from fix and test so each command stays single-purpose.

### T-07.03  Run fix command
id: T-07.03
phase: 7
status: pending
depends_on: [T-03.01, T-04.05, T-06.04, T-01.01]
stack: rust
criteria:
  - C1: builds the fix prompt embedding the `--error` message when provided.
  - C2: absent `--error`, the prompt instructs the model to infer the defect from the file.
  - C3: parsed edits route through output dispatch identically to the edit command.
not_doing:
  - No compiler invocation to obtain the error automatically.
  - No test generation.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The repair subcommand. Inputs: a path, an optional error message, config, and a Completer; bounds: one round-trip producing edits. Outputs: edits routed through dispatch. Errors/edges: a missing error message degrades to inference, not failure. Invariant: edits flow through the same output contract as edit. Done-check: the with-error, without-error, and routing criteria. Fenced from auto-detecting the error so the command needs no compiler.

### T-07.04  Run test command
id: T-07.04
phase: 7
status: pending
depends_on: [T-03.01, T-04.05, T-06.04, T-02.01]
stack: rust
criteria:
  - C1: builds the test prompt and treats the generated tests as a whole-file ParsedEdit.
  - C2: output routes through dispatch, defaulting to a diff against an empty target for a new test file.
  - C3: generated tests target only public-API symbols from the extracted structure.
not_doing:
  - No test execution.
  - No coverage measurement.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The test-generation subcommand. Inputs: a path, config, and a Completer; bounds: produces test code for the public API. Outputs: generated tests routed through dispatch as a whole-file edit. Errors/edges: a not-yet-existing test file diffs against empty. Invariant: only public-API symbols are targeted. Done-check: the whole-file, routing, and public-only criteria. Fenced from running the tests so generation stays deterministic.

### T-07.05  Route subcommand dispatch
id: T-07.05
phase: 7
status: pending
depends_on: [T-00.03, T-07.01, T-07.02, T-07.03, T-07.04, T-01.03]
stack: rust
criteria:
  - C1: the parsed CLI command selects exactly one of the explain/edit/fix/test handlers.
  - C2: the selected handler receives the loaded config and the constructed Completer.
  - C3: an unhandled command variant is a compile error via an exhaustive match.
not_doing:
  - No business logic beyond selection and wiring.
  - No Completer backend choice (default vs live).
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The top-level wiring from parsed command to handler. Inputs: a parsed command, loaded config, and a Completer; bounds: a one-of-four selection. Outputs: invocation of the matching handler. Errors/edges: a new unhandled variant fails to compile rather than silently falling through. Invariant: exactly one handler runs per invocation. Done-check: the selection, injection, and exhaustiveness criteria. Fenced to dispatch so handler logic lives in its own task.

### T-08.01  Drive mock end-to-end tests
id: T-08.01
phase: 8
status: pending
depends_on: [T-07.05, T-01.02]
stack: rust
criteria:
  - C1: an end-to-end test runs `edit` with a MockCompleter and asserts the on-disk file after `--apply`.
  - C2: a dry-run test asserts the target file is unchanged and a diff is emitted.
  - C3: a `--json` test asserts the emitted JSON matches the expected structure.
  - C4: no end-to-end test performs network I/O.
not_doing:
  - No real LLM calls.
  - No subcommand left without at least one end-to-end test.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The full-stack determinism guarantee. Inputs: scripted MockCompleter responses driving the assembled CLI; bounds: the three output modes. Outputs: assertions over disk, stdout diff, and JSON. Errors/edges: the no-network criterion fences out non-determinism. Invariant: every run is reproducible from the script. Done-check: the apply, dry-run, json, and no-network criteria. This is the acceptance gate proving the deterministic layers compose correctly.

### T-08.02  Implement HTTP Completer
id: T-08.02
phase: 8
status: pending
depends_on: [T-01.01, T-01.03]
stack: rust
criteria:
  - C1: a `HttpCompleter` implements `Completer` by POSTing the prompt to the configured endpoint.
  - C2: it reads model and endpoint from `Config`.
  - C3: a non-2xx HTTP response yields `MendError::Completer`.
not_doing:
  - No streaming responses.
  - No retry or backoff.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The real backend behind the trait. Inputs: a prompt, plus model and endpoint from config; bounds: one HTTP POST per call. Outputs: the model's completion text or a `Completer` error. Errors/edges: any non-2xx status is a `Completer` error. Invariant: the rest of the system depends only on the trait, never on this type. Done-check: the POST, config-read, and non-2xx criteria. Fenced from streaming and retries to keep it minimal and behind a feature later.

### T-08.03  Gate live integration test
id: T-08.03
phase: 8
status: pending
depends_on: [T-08.02]
stack: rust
criteria:
  - C1: a `#[cfg(feature = "live")]` test exercises `HttpCompleter` against a real endpoint.
  - C2: the live test is excluded from the default `cargo test` run.
  - C3: the `live` feature is declared in Cargo.toml and is off by default.
not_doing:
  - No CI configuration.
  - No credential provisioning or secret storage.
test_files: []
criteria_map: {}
attempts: 0
last_failure: ""
---
The feature-gated manual integration check. Inputs: a real endpoint reachable only when the feature is on; bounds: a single manually-run test. Outputs: a pass/fail against a live model. Errors/edges: by default the test does not compile into the suite, keeping `cargo test` network-free. Invariant: default test runs never touch the network. Done-check: the gated-test, default-exclusion, and feature-declaration criteria. Fenced from CI and secrets, which are out of scope for the build.
