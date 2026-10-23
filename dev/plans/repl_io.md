# Plan: line input for the rustlab REPL

**Status:** approved and implemented in 0.3.14 (2026-10-22). Michael approved every recommended answer to the open questions, the Security section, and no callbacks.
**Current phase:** complete
**Created:** 2026-10-21
**Request:** Michael — a REPL for `.rlab` scripts, with a small, easy IO surface so the LLM example can run as an interactive chat. Look at MATLAB and other languages, then present a plan.
**Workspace at plan time:** `0.3.13` (`a124c01`). Shipped as `0.3.14`. Line numbers below are anchors from the plan; the code has moved.

This document is the design. Implementation follows Workflow Rules 2, 3, 5, 6, 7, and 12 in the same commits as the code: tests, `docs/functions.md`, `docs/quickref.md`, REPL `HelpEntry` + one toolbox row, `AGENTS.md`, and `CHANGELOG.md`. A behavior change bumps the workspace version if `0.3.13` has already shipped.

---

## Recommendation

Keep the REPL that already ships. Add four MATLAB-shaped ways for a **script** to read a line or pause for commands:

| Form | What the user types | What the script gets |
|---|---|---|
| `s = input("you: ", "s")` | a line of text | that line as a string |
| `x = input("scale: ")` | one expression | the value of that expression |
| `line = fgetl(stdin)` | a raw line, no prompt | the line, or `-1` at end of input |
| `keyboard` | rustlab statements | nothing; `return` resumes the script |

`input(..., "s")` is the chat loop. `keyboard` is the mid-script pause. `fgetl(stdin)` is the pipe-friendly reader. Expression `input` accepts one pure math expression (`2*pi`, `sin(k)`). It does not run assignments, file or IO builtins, `run` / `eval` / `source`, or function definitions. Text from an LLM or a file uses the string form or `fgetl` (§4).

Printing already exists (`disp`, `fprintf`, `print`, `sprintf`). File formats already exist (`save` / `load` for `.npy`, `.csv`, `.toml`, `.npz`). This plan does not add `fopen` or a general file-handle type.

Core math stays pure Rust. No new numeric crate. `rustyline` is already the CLI line editor; it stays in `rustlab-cli`. `input` and `fgetl` read with `std::io` so the script crate gains no dependency.

---

## 1. What rustlab already has

### 1.1 The REPL is already the default command

`rustlab` with no arguments starts the interactive session. `rustlab repl` is the same command (`crates/rustlab-cli/src/cli.rs`: `Commands::Repl`, and `unwrap_or_else` selects it). `docs/agent-guide.md` §2.2 documents it.

The session lives in `crates/rustlab-cli/src/commands/repl.rs` (`execute`). One `Evaluator` lasts until exit, so variables, user functions, and figures survive from line to line and across `run file.rlab`.

| Ergonomic | Today |
|---|---|
| Line editing, history search | `rustyline` 14 (`crates/rustlab-cli/Cargo.toml`, feature `derive`). Prompt `>> `, continuation `.. `. Ctrl-C cancels the line. Ctrl-D or `exit` / `quit` leaves. |
| History file | `~/.rustlab_history`, newest 1000 lines (`[repl] history_limit` in `~/.rustlabrc`, `DEFAULT_HISTORY_LIMIT`). In-session `history`, `history n`, `history clear`. Up/down and Ctrl-R. |
| Tab completion | `ReplHelper`: workspace names, builtins, `help` topics, and file paths after `run` / `ls` / `cd` or inside an open quote. |
| Workspace commands | `help` / `?`, `whos`, `clear` (keeps built-in constants), `pwd`, `cd`, `ls`. |
| `run script.rlab` | The REPL command reads the file and `exec_stmt`s it on the **same** evaluator, so definitions and variables land in the live workspace. The language statement `run "file.rlab"` does the same (`StmtKind::Run` in `eval/mod.rs`). |
| Multi-line | **At plan time:** typed continuation ran only when the first line started with `function`. A keyword helper, `line_opens_block`, recognised `if` / `for` / `while` / `switch` but the REPL did not call it except from its own tests. That helper was removed after `statement_ready` became the continuation check. |
| `ans` | Bare expressions print their value (`StmtKind::Expr` in `eval/mod.rs`). Nothing binds `ans`. Roadmap item B5 (`dev/plans/roadmap.md`). |
| `run` and the script directory | `rustlab run file.rlab` `chdir`s to the script's parent for the process (`commands/run.rs`). The REPL `run` command does not, and the `run` statement does not. Relative `load` / `save` then follow the REPL's current directory. The path jail is unset in the REPL and in `rustlab run` (`path_jail.rs`), so those paths are not sandboxed there. |
| Plots | `--viewer` / `[viewer] auto_connect` at startup. Pending terminal-skip warnings drain after each statement. |

`break` and `continue` stay unsupported on purpose (`dev/requests/break-continue.md`). A chat loop puts the stop test in the `while` header.

### 1.2 IO builtins that exist

Registered in `BuiltinRegistry::with_defaults` (`crates/rustlab-script/src/eval/builtins.rs`):

| Builtin | Role |
|---|---|
| `print(...)` | Space-separated values, then a newline. |
| `disp(x)` | One value, always a newline. |
| `fprintf(fmt, ...)` | C-style format to stdout. Specifiers and flags are documented in `docs/functions.md`. A vector is an error (no recycling). |
| `sprintf(fmt, ...)` | Same format, returns a string. |
| `commas` | Thousands separators, returns a string. |
| `format` | Display mode for echoed values (`short`, `long`, `hex`, `commas`). |
| `save` / `load` / `whos` | `.npy`, `.csv`, `.toml`, `.npz`. Numeric arrays, structs, Touchstone `.sNp` on the `save` path for S-parameters. |
| `audio_read` / `audio_write` | Binary f32-LE PCM on process stdin / stdout. Clean EOF is `ScriptError::AudioEof`, which `rustlab run` turns into exit 0. |

There is no `input`, `keyboard`, `fgetl`, `fgets`, `fopen`, `fclose`, `fread`, `fwrite`, `fileread`, or `printf`. The cache purity list in `crates/rustlab-script/src/purity.rs` (`IMPURE_BUILTINS`) already **reserves** `input`, `keyboard`, `fopen`, `fclose`, `fread`, `fwrite`, and `printf` so a future builtin is excluded from the persistent cache. Those names are not registered, so a script that calls `input` today is an "undefined function" error.

Stdout from `disp` / `fprintf` / `print` / expression echo goes through `eval/output.rs`. When capture is on (notebook execution), the text is appended to a thread-local buffer and the notebook shows it. The REPL and `rustlab run` print to the process stdout.

### 1.3 Where the LLM example hurts

`rustlab_llm` is a sibling repo. It is not in this checkout. This repo's perf plans name the model code as `../rustlab_llm/lib/gpt.rlab` (`gpt_forward`, `gpt_backward`, `gpt_unflatten`, `gpt_sinusoidal_pe`) and the driver as a batch perf harness (`make perf-check`). Roadmap A5 ("Lesson 9: LLM error") is a separate, still-vague notebook bug; this plan does not claim to fix it.

What that shape of program can do today:

- Print a reply with `fprintf` / `disp` / `print`.
- Load weights with `load` if they are `.npy`, `.csv`, `.toml`, or `.npz`.
- Define the step functions in a file and `run` them into the REPL workspace.

What it cannot do:

- Read the next user message. A prompt has to be a string literal in the source, or a field inside a loaded file. There is no line reader.
- Pause inside `gpt_forward` and inspect activations, then resume.
- Type a `while` chat loop at the `>>` prompt one line at a time. The loop has to be pasted as a whole block, or written in a file and `run`.

A chat loop wants this shape (the `respond` function is the caller's; rustlab does not add it):

```rlab
run "lib/gpt.rlab"
model = load("weights.toml")

line = input("you: ", "s")
while length(line) > 0
  reply = respond(model, line)
  fprintf("model: %s\n\n", reply)
  line = input("you: ", "s")
end
```

Empty input ends the loop, which matches the language's "no `break`" rule. End of a pipe ends the process cleanly (question 2).

The LLM example uses string mode (`input(prompt, "s")` or `fgetl(stdin)`). The line is data. rustlab does not parse it and does not execute it. An API key, if the sibling repo reads one from the environment, stays in that variable. No IO builtin prints the environment, and the example must not `disp` or `fprintf` the key. This plan does not call a paid API. A later change that does needs Michael's explicit approval (§4).

`keyboard` is the debug pause, inside their function:

```rlab
function y = respond(model, line)
  tokens = encode(line)
  keyboard
  y = decode(gpt_forward(model, tokens))
end
```

At `K>>` the function locals are visible. `return` resumes `respond`. This plan does not modify `rustlab_llm`. Phase 5 adds a tiny echo script in **this** repo that has the same loop, so the IO can be tried without a model.

---

## 2. Prior art, briefly

The ergonomics that matter for this request: line editing, history, multi-line blocks, a workspace that survives `run`, a last-result name, tab completion, and a way for a **running script** to read a line or drop to a prompt.

| Language | Session | Reading a line from a script | Dropping into the session mid-script |
|---|---|---|---|
| MATLAB | Command Window, `>>`. History, tab completion, `...` continuation, multi-line `if`/`for`/`while`/`function` until `end`. `ans` is the last unassigned expression. `run script.m` executes in the base workspace. | `input(prompt)` evaluates one expression. `input(prompt,'s')` returns the text. | `keyboard` pushes a `K>>` prompt in the caller's workspace. `return` or `dbcont` resumes. `dbquit` aborts the debug session. `dbstop` is a breakpoint facility and is out of scope here. |
| Octave | CLI built on GNU readline when configured. `~/.octave_hist`. Same block structure and `ans` as MATLAB. `source` / `run` fill the workspace. | `input(prompt)` and `input(prompt, "s")`. `fgetl(stdin)` reads one line and drops the newline; `-1` means end of input. `stdin` is a stream value. `fgets` keeps the newline. | `keyboard`, same idea as MATLAB. |
| Julia | `julia>` REPL with modes (help, shell, package). Incomplete parses continue the prompt. History under `~/.julia`. `include("file.jl")` runs into `Main`. `ans` is the last unassigned result. | `readline()` returns a `String`. | A nested REPL is possible (`REPL.jl`); the everyday equivalent of `keyboard` is less central than in MATLAB. |
| Python | CPython REPL (readline or libedit). Incomplete blocks use `...`. No `ans` (`_` in IPython). `exec(open(path).read())` or IPython `%run` fills a namespace. | `input(prompt)` returns a `str`. EOF raises `EOFError`. | `code.interact(local=globals())` and `pdb.set_trace()` embed a prompt in the live locals. |
| R | `>` prompt, readline history and completion. `source("file.R")` can run in the global environment. | `readline(prompt)` returns a length-1 character vector. `readLines("stdin", n=1)` is the stream form. | `browser()` nests a prompt in the current frame. |
| evcxr | Rust REPL on `rustyline`. Incomplete Rust continues the prompt. History and a growing workspace of items. | Ordinary `std::io::stdin().read_line`. No MATLAB-style `input`. | The REPL **is** the session; there is no separate "script dropped into the REPL" builtin. |

What to copy, and what to leave:

- Copy MATLAB's two `input` forms and Octave's `fgetl(stdin)` / `-1` on EOF. They are the smallest API a MATLAB user will type correctly the first time.
- Copy `keyboard` / Python `code.interact` / R `browser`: one call, the live workspace, an explicit resume word.
- Copy the incomplete-block continuation from Julia and Python, using the block keywords rustlab already has. A full incremental parser is unnecessary.
- Leave `dbstop`, Julia's mode prompts, IPython magics, and evcxr's `:dep` commands. They are separate products.
- Leave `fopen`. A file-id table is a larger design (text vs binary, path jail, who closes the handle). `load` / `save` already cover the numeric files the model uses.

---

## 3. Design

### 3.1 REPL continuation (Phase 1)

Change the gate in `repl.rs` so **every** submitted line runs through one completion check, not only lines that start with `function`.

`fn statement_ready(buf: &str) -> bool` in `rustlab-script` (unit-tested, no TTY). The shipped check tokenizes the buffer (block keywords, `...`, and `[]` / `()` / `{}`, with strings left to the lexer). It does not call `line_opens_block`; that helper was removed because nothing in the REPL consulted it.

1. An open `function` / `if` / `for` / `while` / `switch` (depth greater than 0) means keep reading.
2. A line whose trimmed text ends in `...` means keep reading (the lexer already treats `...` as continuation inside a file).
3. Unclosed `[]`, `()`, or `{}` means keep reading. A string is lexed, so a bracket inside quotes does not flip the count.

When the check says the buffer is unfinished, prompt with `.. ` and append. Bracketed paste still works: the whole paste is fed to the same check before any continuation read.

A balanced one-liner (`if x > 0, y = 1; end`) has depth 0 and is ready. A line that is only `while true` is not.

Parse errors on a **ready** buffer still print `error:` and return to `>> `. The continuation check is a heuristic. It does not call the parser, so a typo does not trap the user in `.. `.

### 3.2 `ans` (Phase 2)

In the `StmtKind::Expr` arm, when the value is printed today (`!suppress`, not inside a user function, not `Value::None`), also `set_env("ans", val)`.

- `1 + 2` prints and sets `ans` to `3`.
- `x = 1 + 2` echoes `x = ...` and leaves `ans` alone.
- `1 + 2;` prints nothing and leaves `ans` alone.
- Inside a `function` body, bare expressions stay silent, and `ans` stays unchanged. `keyboard` in that function still sees the locals the function assigned.

`ans` is an ordinary variable. `clear` removes it. It is not a built-in constant.

### 3.3 `stdin`, `input`, `fgetl` (Phase 3)

**`stdin`.** A unit variant `Value::Stdin`, installed with the other built-in constants (`i`, `j`, `pi`, …) so `clear` restores it. Display is `stdin`. Arithmetic on it is a type error. `fgetl` accepts only this value.

**Where the call runs.** `input` and `keyboard` need the live `Evaluator` (expression `input` evaluates the line; `keyboard` executes statements). A `BuiltinFn` is `fn(Vec<Value>) -> Result<Value, ScriptError>` and has no evaluator. Intercept `input`, `fgetl`, and `keyboard` in the `Expr::Call` arm in `eval/mod.rs`, next to `profile` and `widget`, and also register them so `feval("input", ...)` hits the same functions. `feval` already runs inside the evaluator, so the special case covers it.

**`input(prompt)` / `input(prompt, "s")` / `input()`.**

- Zero arguments or one argument: print the prompt with `script_print` (no extra newline), read one line, strip the trailing newline, and evaluate it only if the pure-expression check in §4 accepts it. Variable reads see the current scope (function locals, or the base workspace at the prompt), so `2*k` can see a local `k`. A rejected line is a runtime error and changes nothing in the workspace.
- A second argument that is the string `"s"`: return the line as `Value::Str`. Do not parse it and do not execute it. This is the form the LLM chat loop uses. A blank Enter is `""`. Untrusted text (a pipe, a file, or a model reply) uses this form or `fgetl`.
- Any other second argument is a type error.
- The prompt is printed only for `input`. It goes to the same stream as `fprintf` (stdout, or the notebook capture buffer — but notebook context refuses before the prompt; see §4).

**`fgetl(stdin)`.** No prompt. Returns the line as a string without the newline. A blank line is `""`. End of input returns the scalar `-1` (Octave / MATLAB). The loop idiom, once `ischar` exists:

```rlab
line = fgetl(stdin)
while ischar(line)
  disp(line)
  line = fgetl(stdin)
end
```

`ischar(x)` is a pure predicate: `true` for a string, `false` otherwise. It exists so the EOF sentinel has a readable test. It is not impure and is safe inside `parmap`.

`fgetl()` with no arguments is an error that names `fgetl(stdin)`. `fgetl` of anything other than the `stdin` value is an error that says file handles are not available.

**End of input on `input`.** A closed stdin, or Ctrl-D when the reader reports EOF, raises `ScriptError::InputEof`. `rustlab run` maps it to exit 0 with no `error:` line, the same way it maps `AudioEof`. The REPL maps it to a short `(eof)` note and stays at `>> ` (question 6). A chat script whose first `input` hits `/dev/null` therefore exits 0, which is what `make examples` needs.

**Line source.** A thread-local reader:

- Unset: `stdin().read_line`.
- Set: a queue of lines, used by tests. `None` from the queue is EOF.

The production reader is `std::io`. It does not link `rustyline`. Backspace still works in the terminal's cooked mode. History of previous **answers** is a later hook (question 9), not part of v1.

### 3.4 `keyboard` (Phase 4)

`keyboard` with no arguments pauses the script and reads statements until the user resumes.

Resume words, matched on the trimmed line **before** execute, so they are not the language `return` statement:

| Line | Effect |
|---|---|
| `return` | Resume the caller. A `return` inside a function is not taken. |
| `dbcont` | Same as `return`. |
| `dbquit` | `ScriptError::runtime("keyboard: dbquit")`. The script stops. `rustlab run` exits 1. |

`exit` / `quit` at `K>>` are an error that tells the user to type `return`. They do not kill the process. At the top-level `>> ` prompt, `exit` still quits.

The nested prompt is `K>> `, then `K2>> ` if `keyboard` is entered again. Continuation uses the same `statement_ready` helper as the main REPL, with `.. `.

**Who reads the line.**

- `rustlab-script` defines `set_keyboard_handler`. The handler is `FnMut(&mut Evaluator) -> Result<KeyboardStatus, ScriptError>` in a thread-local.
- The CLI REPL installs a handler that uses the **same** `rustyline` `Editor` (editing, history, completion). `keyboard` lines are rustlab statements, so they go into `~/.rustlab_history`. `input` lines do not.
- When no handler is installed (`rustlab run` on a TTY), `rustlab-script` runs a small loop: print `K>> `, `read_line`, parse, `exec_stmt`. No new dependency.
- When stdin is not a terminal, `keyboard`, `dbcont`, and `dbquit` error immediately (`keyboard: no interactive terminal`), including when a handler is installed. A piped script cannot enter the debugger. The handler is not called. See §4.

Assignments and expressions at `K>>` run on the same evaluator, so a pause inside `respond` sees `model`, `line`, and `tokens`, and an assignment there is still there after `return`.

### 3.5 REPL `run` and the script directory (Phase 1)

When the path jail is unset, both the REPL `run` command and `StmtKind::Run` save the process cwd, `chdir` to the script's parent, execute, and restore the cwd. Relative `load("weights.toml")` then resolves next to the script, which is what `rustlab run` already does for the whole process.

When a jail is installed (notebooks), `run` stays as it is: `check_path`, no `chdir`. The jail's `base` is the path root, and cwd is process-global.

### 3.6 How the pieces sit

```
>> user types a chat loop          rustyline in rustlab-cli
        |  statement_ready
        v
   Evaluator::exec_stmt
        |
        +-- input / fgetl  --> std::io line (or the test queue)
        |                      expression input: one pure math expr (§4)
        |                      "s" and fgetl: raw string, never executed
        |
        +-- keyboard -------> handler if the CLI installed one
        |                      else std::io loop on a TTY
        |
        +-- disp / fprintf -> eval/output.rs (stdout or notebook capture)
```

`rustlab-dsp` and `rustlab-core` stay untouched.

### 3.7 Docs and help when the code lands

Same commit as the builtins:

- `docs/functions.md` — `input`, `fgetl`, `keyboard`, `ischar`, the `stdin` constant, and the chat-loop example. A sentence that `audio_read` and these readers share process stdin, so one script uses one of them.
- `docs/quickref.md` — the same names under Language / IO.
- `docs/agent-guide.md` §2.2 — continuation, `ans`, `input`.
- `docs/security.md` — the §4 rules: pure-expression `input`, string mode for untrusted text, notebook / watch / `parmap` refusal, non-TTY `keyboard`, history file mode `0600`, and no IO builtin printing the environment.
- `crates/rustlab-cli/src/commands/repl.rs` — `HelpEntry` for each new name, all in the existing language toolbox row that already holds `disp` / `fprintf` / `print` (one row only; `help_coverage_tests` enforces that).
- `AGENTS.md` — the builtin table, and a line in the REPL crate notes.
- `CHANGELOG.md` — *Added* for the builtins and continuation; *Breaking / behavior changes* only if `ans` or the `run` cwd change can affect an existing script (a script that already used the name `ans`, or a REPL `run` that depended on the old cwd). Migration: rename a variable called `ans`; pass absolute paths if a REPL `run` must keep the old directory.

---

## 4. Security

### 4.1 Evaluate-mode `input` is one pure expression

`input(prompt)` and `input()` parse the line as a single expression and then walk that AST. The walk is an allowlist. The line runs only when every node is one of:

- a literal (number, imaginary literal, string, `true` / `false`)
- a variable read, including built-in constants (`pi`, `k` in scope)
- unary minus, the arithmetic and comparison operators, or `'` / `.'`
- an index read of a variable (`v(1)`, `M(i, j)`) whose subscripts are themselves pure
- a call whose name is a **registered builtin** and is **not** on the impure list

The impure list is the union of `purity.rs`'s `IMPURE_BUILTINS` and a fixed extra set of names that must fail even if they are not registered yet: `run`, `eval`, `feval`, `source`, `system`, `unix`, `getenv`, `setenv`, `input`, `keyboard`, `fgetl`, `fopen`, `fclose`, `fread`, `fwrite`, `save`, `load`. A user function, a lambda, and a function handle are rejected. User functions can close over IO, so they are not "pure" for this check.

Rejected, with a runtime error and no workspace change:

- an assignment, a compound assignment, or an index assignment (`x = 1`, `x += 1`, `v(1) = 2`)
- a function definition
- `run`, `eval`, `feval`, `source`
- a file, system, or IO builtin (`save`, `load`, `fprintf`, `disp`, `print`, `system`, …)
- more than one statement, or a `for` / `while` / `if` / `switch` block

`2*pi` and `sin(k)` are accepted. `save("k.npy", x)` is not.

Untrusted text — a line piped from a file, or text that came from an LLM — uses `input(prompt, "s")` or `fgetl(stdin)`. Both return a raw string. The evaluator does not parse that string and does not execute it. The LLM example must use string mode. The echo script in Phase 5 uses `input("you: ", "s")`.

### 4.2 Notebooks, `watch`, `parmap`, and a non-TTY debugger

| Context | `input` / `fgetl` | `keyboard` / `dbcont` / `dbquit` | Paths |
|---|---|---|---|
| REPL, and `rustlab run` on a TTY | Read stdin. Expression form follows §4.1. String form and `fgetl` return text. | REPL: rustyline handler. `rustlab run` on a TTY: std loop. `dbcont` and `dbquit` are resume and abort lines inside that session. | Jail unset. Phase 1 cwd dance for `run` only. |
| `rustlab run` with stdin not a TTY, including `--plot none` and `make examples` | String form and `fgetl` still read the pipe. Evaluate-mode `input` still has to pass §4.1. `InputEof` on a closed stdin exits 0. Headless means "no TUI plot", not "no stdin". | **Error immediately.** `keyboard`, `dbcont`, and `dbquit` all fail with `keyboard: no interactive terminal` before any debug line is read. An installed handler is not called. A piped script cannot enter the debugger. | Same as `rustlab run`. |
| Notebook `render` / `watch` (`PlotContext::Notebook`) | Hard error before any read: `input: not available while rendering a notebook`. Do not block the render. Do not read process stdin. `--stdin` on `render --format json` is the markdown source, and a cell must not consume it. | Same hard error. | Existing jail. No `chdir`. |
| `parmap` worker | Hard error via `require_pure_context`: `parmap: cannot input from a parallel lambda`. Add `input`, `fgetl`, and `keyboard` to the **runtime** denylist in `eval/mod.rs` (`IMPURE_BUILTINS` there today lists `fprintf` and the plot commands, and does not list `input`). Add `fgetl` to the cache denylist in `purity.rs` (`input` and `keyboard` are already on it). | Same. | Workers already install the caller's jail. These builtins open no paths. |
| Persistent function cache | A function that calls `input`, `fgetl`, or `keyboard` is impure and is not cached. The walker is name-based and already lists `input` and `keyboard`. | Same. | Unchanged. |

`ischar` is pure. It is not added to either denylist.

Widgets stay the interactive control for notebooks (`widget()`). This plan does not route `input` through the browser.

`audio_read` also locks stdin and reads binary frames. v1 documents that a script uses either PCM stdin or line stdin. There is no interleaving lock (question 8).

No shell is spawned. No new path is accepted by `input` or `fgetl`.

### 4.3 History file mode, and turning history off

REPL command history is `~/.rustlab_history` (rustyline). Prompts and `keyboard` lines can contain secrets, so the file is owner-only.

- After every create and every save, set the mode to `0600` (`std::fs::set_permissions`). Do this on startup as well when the file already exists, so a file left group-readable from an older build is tightened.
- `input` and `fgetl` lines are not written to the history file (question 7). `keyboard` statements are, because they are rustlab commands.
- History can be turned off. Either one is enough, and the environment variable wins when it asks for off:
  - `[repl] history = false` in `~/.rustlabrc` (next to the existing `history_limit`)
  - `RUSTLAB_NO_HISTORY=1` in the environment
- When history is off, do not load the file and do not create or rewrite it. The in-session `history` command lists only lines from this process, and those lines are discarded on exit.

### 4.4 Environment variables and paid APIs

No IO builtin prints the environment. `disp`, `print`, `fprintf`, `sprintf`, `input`, and `fgetl` write only the values the script passed them. This plan does not add `getenv` / `setenv`. There is no builtin that dumps `std::env::vars`.

The LLM example reads its API key from the environment inside the sibling repo's own code, and it never echoes or logs that value. The echo script in this repo does not read a key and does not make a network call. A change that makes a paid API call needs Michael's explicit approval in the request that asks for it. This plan does not grant that approval.

---

## 5. Dependency trade-off: `rustyline`

`rustyline` is infrastructure (a line editor), not core math. Rule 10's "hand-roll the numerics" rule is unchanged. No trade-off study file is required **because no crate is added**.

### What we already link

- Crate: `rustyline` 14, feature `derive`, dependency of `rustlab-cli` only.
- License: MIT, compatible with `MIT OR Apache-2.0`.
- Role today: REPL editing, history, hints, completion. The script library, the notebook binary, and `rustlab run` do not link it for reading.

### What v1 does

| Piece | Reader | Why |
|---|---|---|
| `input`, `fgetl` | `std::io::BufRead::read_line` | Works in pipes and unit tests. No line-editor crate in `rustlab-script`. Cooked-mode terminals already edit the line. |
| `keyboard` under `rustlab repl` | The CLI's existing `Editor`, via `set_keyboard_handler` | Editing, history, and completion at `K>> ` without a second copy of the REPL loop's behavior. |
| `keyboard` under `rustlab run` on a TTY | `read_line` loop inside `rustlab-script` | A script can pause on a terminal with zero extra dependencies. |
| `keyboard` with no TTY and no handler | Immediate error | CI and notebooks do not block. |

### No-dependency fallback

If the handler is unset, every new read is `std::io`. Deleting the handler still leaves `input`, `fgetl`, and TTY `keyboard` working. The REPL's own `>> ` prompt keeps `rustyline`, which it already uses.

### What we will not do

- Move `rustyline` into `rustlab-script` or `rustlab-core`.
- Add another line-editor crate.
- Use `rustyline` to read `input()` in v1. A host hook can come later if answer-editing turns out to matter (question 9).

---

## 6. Open questions

Each recommendation is the default if the review says yes to the plan as a whole. A different answer changes the phase noted on that row.

1. **Expression `input(prompt)` is one pure expression (§4.1).** Decided with this update. Literals, variable reads, math operators, and pure math builtins only. Assignments, file/system/IO builtins, `run` / `eval` / `feval` / `source`, and function definitions error. Untrusted text uses `input(prompt, "s")` or `fgetl(stdin)`, which return a raw string and never execute. The LLM example uses string mode. v1 does not continue `input` across multiple lines.
2. **EOF.** Recommended: `input` raises `ScriptError::InputEof`, and `rustlab run` exits 0 with no error line (same policy as `AudioEof`). `fgetl` returns the scalar `-1` so the script can close the loop and keep running. A blank Enter is `""` for both, and is not EOF.
3. **`stdin` is a real value.** Recommended: `Value::Stdin`, a built-in constant named `stdin`, restored by `clear`. `fgetl` accepts only that value. A string `"stdin"` is a type error, so a typo is obvious.
4. **`ischar`.** Recommended: yes, in the same phase as `fgetl`. It is the predicate in the `while ischar(line)` loop. `class(line) == "string"` would also work; `ischar` is what the MATLAB examples use, and it is a few lines.
5. **`keyboard` resume words.** Recommended: on a TTY, `return` and `dbcont` resume and are not executed as the language `return`. `dbquit` aborts the script with a runtime error (exit 1 from `rustlab run`). `exit` at `K>>` errors and stays in `keyboard`. `exit` at `>> ` still quits the process. When stdin is not a TTY, `keyboard`, `dbcont`, and `dbquit` error immediately and do not open a debug session (§4.2).
6. **Ctrl-D during `input()` while the REPL is running.** Recommended: deliver EOF to the script (question 2) and return to `>> `. Ctrl-D at `>> ` still quits. Implementing this means the REPL's `input` path must not treat that EOF as "leave the session".
7. **History.** Decided with this update (§4.3). `keyboard` statements are recorded in `~/.rustlab_history`. Lines read by `input` and `fgetl` are not. The file mode is `0600`, tightened on load and on save. `[repl] history = false` or `RUSTLAB_NO_HISTORY=1` skips load and save. The environment variable forces history off when both are set.
8. **`audio_read` and line input in one process.** Recommended: document the mutual exclusion, and do not add a runtime lock in v1. A script that mixes them consumes stdin bytes in whichever order it calls.
9. **Line editing inside `input()`.** Recommended: standard input only, for v1. Revisit a `rustyline` host hook if typing long prompts without cooked-mode editing is a problem. `keyboard` still uses `rustyline` in the REPL.
10. **Notebooks and `watch`.** Recommended: hard error, no blocking read, no browser prompt. Interactive notebook controls stay on `widget()`.
11. **REPL `run` changes directory to the script's folder for the duration, then restores it, only when the path jail is unset.** Recommended: yes, so `run "chat.rlab"` finds `weights.toml` beside the script. Notebook `run` does not `chdir`.
12. **`ans`.** Recommended: yes (Phase 2). Bind it only when a top-level bare expression is printed. Assignments and `;` do not update it. This closes roadmap B5 for that case.
13. **Version.** Recommended: the implementation PR follows Rule 12. If `0.3.13` is already tagged, bump the workspace version in that PR. If it is still the unreleased current version, add the changelog notes under `0.3.13` and do not bump. This plan does not bump anything.
14. **Where the sample loop lives.** Recommended: `examples/language/chat_loop.rlab`, an echo loop (the reply is the line the user typed), with a comment showing the `respond(model, line)` call site. No network, no API key, no `rustlab_llm` checkout. `make examples` runs it with stdin `/dev/null`; `InputEof` exits 0, so it does not need a skip entry and it does not hang.
15. **`fopen` / reading a text file with `fgetl`.** Recommended: out of scope. `fgetl` is stdin-only. Numeric and TOML files stay on `load`. A later plan can add file handles and put them through `check_path`.

---

## 7. Phases

Each phase is shippable on its own. Tests are headless (`cargo test --workspace`). No phase starts without an explicit yes on this plan. After a phase, stop and ask before the next one (the session protocol in `AGENTS.md`).

### Phase 1 — Continuation and `run` cwd

**Status:** complete

- Add `statement_ready` and use it for every REPL line.
- Jail-unset `run` saves, changes, and restores cwd (REPL command and `StmtKind::Run`).
- Tests in `crates/rustlab-script`: `while` / `for` / `if` / `switch` depth, a `for` inside a `while`, a one-line `if`, a trailing `...`, an unclosed `[`, a quote that contains a bracket, a balanced paste, a similar identifier (`whilelist`) that must not open a block. The old `line_opens_block` helper was not used once `statement_ready` owned continuation, and it was removed (the helper and the tests that only called it).
- History security tests (temp `HOME`, no real `~/.rustlab_history`): a new history file is mode `0600`; an existing `0644` file is tightened to `0600` on save; `RUSTLAB_NO_HISTORY=1` does not create the file and does not rewrite an existing one; `[repl] history = false` does the same; when both the rc enables history and the env var is set, the file stays untouched.
- A script-crate test that `run` of a temp file `load`s a relative `.toml` from the script directory while the process cwd is elsewhere, and that the cwd is restored. Notebook jail test: relative `run` still cannot escape the jail, and cwd is unchanged.

### Phase 2 — `ans`

**Status:** complete

- Bind `ans` as in §3.2.
- Tests: bare expression sets `ans`; assignment does not; `;` does not; a later bare expression replaces it; inside a function the outer `ans` is unchanged.
- Changelog note for scripts that already used the name `ans`.

### Phase 3 — `stdin`, `input`, `fgetl`, `ischar`

**Status:** complete

- `Value::Stdin`, constant, Display, arithmetic error.
- Call-arm intercept plus registry entries.
- Test line queue.
- `InputEof` mapped to exit 0 in `commands/run.rs` and in `run_script_source` (the REPL's script runner), matching `AudioEof`.
- Notebook context: error, and the test asserts stdin was not read (empty queue still full, or a reader that panics if called).
- `parmap`: error naming `input` and `fgetl`.
- Cache walker: `fgetl` is impure; a fixture function that calls it is rejected. `ischar` is accepted.
- Tests for the behaviors in §3.3: `"s"` does not evaluate `1+2`; the expression form returns `3` for the line `1+2` and sees a local; blank line is `""`; `fgetl` EOF is the scalar `-1`; `ischar("-1")` is true and `ischar(-1)` is false; bad arity; `fgetl("stdin")` errors.
- Security tests for §4.1, on the injected line queue, with a temp directory as cwd so a slipped `save` would leave a file:
  - Accept `2*pi` and `sin(k)` when `k` is in scope.
  - Reject `x = 1`, `v(1) = 2`, `function y = f(x)`, `run "t.rlab"`, `feval("save", "k.npy", x)`, `eval("1")`, `source("t.rlab")`, `save("k.npy", x)`, `load("k.npy")`, `fprintf("%s\n", "x")`, `system("true")`, and a call to a user function. Workspace is unchanged and no file appears.
  - `input(prompt, "s")` given the text `save("k.npy", x)` returns that string and does not create `k.npy`. `fgetl(stdin)` of the same text does the same.
  - `disp`, `print`, `fprintf`, and `sprintf` do not read or print the process environment. Feed a unique `RUSTLAB_TEST_SECRET` in the environment, run each builtin on an unrelated value, and assert the captured output does not contain the secret. There is no `getenv` registration.
- `watch` uses the same `PlotContext::Notebook` refusal as `render`. One test sets that context, calls `input`, `fgetl`, and `keyboard`, and asserts the line reader was not called.
- Docs, help, quickref, agent guide, security note, changelog, version rule.

### Phase 4 — `keyboard`

**Status:** complete

- Handler thread-local, std fallback, non-TTY error.
- CLI handler sharing the REPL `Editor` and `statement_ready`.
- Tests with a fake handler, on a simulated TTY: one assignment, then `return`; locals visible inside a function after resume; `dbquit` errors; `exit` at the nested prompt errors and the handler can resume; nested `keyboard` prompts `K2>>` (assert the prompt string the fallback would print); `parmap` and notebook refuse.
- Non-TTY security tests (§4.2): with the TTY flag forced off, `keyboard`, `dbcont`, and `dbquit` each return `keyboard: no interactive terminal`. Install a handler that fails the test if it is called, and assert it is not called. A piped script cannot enter the debugger.
- A CLI unit test that the resume words are classified without executing `return` as `StmtKind::Return`. That classifier is a pure function next to `statement_ready`.

### Phase 5 — Echo chat example

**Status:** complete

- `examples/language/chat_loop.rlab` as in question 14.
- Do not register it in `crates/rustlab-cli/tests/examples.rs` (that harness has no stdin). `make examples` covers the EOF-exit path once Phase 3's `InputEof` policy is in.
- One script-crate test runs the same loop against the line queue `["hello", ""]` and checks the captured `fprintf` text. Capture is `eval/output.rs`.
- Comment in the example points at `respond(model, line)` and at this plan. No weights, no network, no API key, no paid call.
- Security tests for the example source: every `input(` call is the string form (`"s"`); the file does not call `getenv` and does not mention printing an API key. A script-crate test runs the loop in string mode against the queue `["save(\"k.npy\", x)", ""]` and asserts the reply text is that line and that no `k.npy` was written. The LLM example, when it exists in the sibling repo, follows the same rule: string mode, key read from the environment, key never echoed. A paid API call is out of this plan until Michael approves it in a later request.

---

## 8. Files an implementation will touch

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-cli/src/commands/repl.rs` | 1, 4 | `statement_ready`, continuation gate, history mode `0600` and the off switch, `keyboard` handler, help entries |
| `crates/rustlab-config/src/lib.rs`, `docs/rustlabrc.example.toml` | 1 | `[repl] history` boolean |
| `crates/rustlab-cli/src/commands/run.rs` | 3 | `InputEof` → exit 0 |
| `crates/rustlab-script/src/eval/mod.rs` | 2, 3, 4 | `ans`, call intercept, runtime denylist, `run` cwd |
| `crates/rustlab-script/src/eval/value.rs` | 3 | `Value::Stdin` |
| `crates/rustlab-script/src/eval/builtins.rs` | 3, 4 | register `input`, `fgetl`, `keyboard`, `ischar`; constant `stdin` |
| `crates/rustlab-script/src/error.rs` | 3 | `InputEof` |
| `crates/rustlab-script/src/purity.rs` | 3 | add `fgetl` to the cache denylist |
| `crates/rustlab-script/src/tests.rs` | 2–5 | behavior tests |
| `docs/functions.md`, `docs/quickref.md`, `docs/agent-guide.md`, `docs/security.md` | 3, 4 | user-facing |
| `AGENTS.md`, `CHANGELOG.md`, `Cargo.toml` | 3 | table, notes, version rule |
| `examples/language/chat_loop.rlab` | 5 | echo loop |

`crates/rustlab-core` and `crates/rustlab-dsp` are not on this list.

History mode and the off switch also touch `crates/rustlab-cli/src/commands/repl.rs` (save path) and `crates/rustlab-config` (`[repl] history`). `docs/rustlabrc.example.toml` documents the key. Those land in Phase 1.

### Security tests (index)

The cases live in the phase that builds the feature. This list is the checklist.

| Rule | Phase | Test |
|---|---|---|
| Evaluate-mode allowlist; reject assignment, `run` / `eval` / `feval` / `source`, function defs, file/system/IO builtins | 3 | Script tests on the line queue; no file created |
| `input(..., "s")` and `fgetl` return raw text and never execute it | 3 and 5 | Queue contains `save("k.npy", x)`; workspace unchanged; no file |
| LLM / echo example uses string mode only | 5 | Source check plus the queue test above |
| Notebook `render` and `watch` refuse stdin reads; reader not called | 3 | `PlotContext::Notebook` |
| `parmap` refuses `input`, `fgetl`, `keyboard` | 3 and 4 | `require_pure_context` error |
| `keyboard`, `dbcont`, `dbquit` error when stdin is not a TTY; handler not called | 4 | Forced non-TTY |
| History file mode `0600`, including tightening an existing looser file | 1 | Temp `HOME` |
| `RUSTLAB_NO_HISTORY=1` and `[repl] history = false` skip load and save | 1 | Temp `HOME` |
| No IO builtin prints the environment | 3 | `RUSTLAB_TEST_SECRET` absent from captured output; no `getenv` builtin |

---

## 9. Future work / Non-goals

Callbacks were considered and deliberately deferred. An `oninput(@fn)` hook, or a timer whose events are pumped with `waitfor`, would turn the REPL into an event loop shared with notebooks and `parmap`. v1 stays a blocking line read (`input`, `fgetl`) and a blocking pause (`keyboard`). That is enough for the chat loop. Do not add the callback or the pump in these phases.

Also out of scope, as decided above: `fopen` and file handles (question 15), `dbstop`, a `rustyline` reader inside `input()` (question 9), notebook prompts in the browser, and any paid API call. The echo example does not read an API key. Wiring `rustlab_llm` to a paid endpoint waits for Michael's explicit approval.
