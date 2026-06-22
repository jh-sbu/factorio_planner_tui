---
name: debug-factorio-tui
description: Run, exercise, capture, and diagnose the Factorio Planner Ratatui application through realistic keyboard workflows. Use when reproducing an interactive TUI bug, checking what a screen displays, testing plan creation or editing manually, or when a raw PTY fails because Crossterm cannot read the terminal cursor position.
---

# Debug Factorio TUI

Exercise the real binary in an isolated copy of the cached application data. Capture the final terminal screen as plain text so observations can be quoted and converted into regression tests.

## Workflow

1. Read `AGENTS.md`, `README.md`, and the relevant application/TUI tests before running the app. Identify the documented keys and expected screen transition.
2. Run `cargo build` so the harness executes the current binary.
3. Create an isolated writable data directory. The app initializes a rolling log before drawing the TUI, so a read-only normal application-data directory causes a panic.
4. Copy the cached profile into the isolated directory when the scenario needs real Factorio data. Do not mutate the user's profile.
5. Run `scripts/run_tui_scenario.py` with one `--key` argument per interaction.
6. Report the visible result precisely, including errors, empty panes, selected values, and any environment-only launch problem. Separate reproduction observations from inferred root causes.
7. For a requested fix, follow project TDD: add a failing domain or application test, implement the smallest fix, and run `cargo test`.

## Prepare Data

Use a fresh directory while preserving the directory layout expected by the `directories` crate:

```bash
debug_root="$(mktemp -d /tmp/factorio-planner-tui-debug.XXXXXX)"
cp -a "$HOME/.local/share/factorio-planner-tui" "$debug_root/"
```

Pass `--data-home "$debug_root"`. This sets `XDG_DATA_HOME`; the copied application directory remains `$debug_root/factorio-planner-tui`.

If no cached profile exists, import a fixture or real `data.raw` dump into the same isolated data home before exercising the scenario. Prefer real cached data for bugs involving recipe selection or dependency expansion because minimal fixtures may not reproduce them.

## Run A Scenario

From the repository root, create an advanced-circuit plan at one item per second:

```bash
python3 .agents/skills/debug-factorio-tui/scripts/run_tui_scenario.py \
  --data-home "$debug_root" \
  --key j \
  --key enter \
  --key text:Debug-plan \
  --key enter \
  --key text:advanced-circuit \
  --key enter \
  --key text:1 \
  --key enter
```

Supported named keys are `enter`, `esc`, `tab`, `backtab`, arrows, `backspace`, `delete`, and `ctrl-c`. Use `text:VALUE` for literal text; a one-character value sends that key directly. Adjust `--rows`, `--cols`, or `--wait` only when a screen needs more space or slower transitions.

The harness uses a real pseudo-terminal, answers Crossterm's cursor-position query, interprets the ANSI redraw stream, prints the final screen, and terminates the child. Prefer it to a direct PTY command when the app exits with `The cursor position could not be read within a normal duration`.

## Diagnose The Result

- Confirm each prompt transition indirectly from the final values; rerun shorter prefixes when a key may have landed on the wrong screen.
- Treat the currently highlighted start action as significant. The start screen initially selects `Import data`, so creating a plan requires moving down once before pressing Enter.
- Preserve exact identifiers and cycle paths from calculation errors.
- Inspect logs under `$debug_root/factorio-planner-tui/logs` when the screen is insufficient.
- Kill or close every launched process. The harness does this even on errors; avoid leaving detached terminal sessions running.
- Remove the temporary data directory after gathering any needed logs.

Do not claim that an environment-only read-only logging failure is an application workflow bug. Do call out an application panic if it also occurs with a writable isolated data home.
