# Factorio Planner TUI

A keyboard-first terminal application for calculating Factorio factory production
rates. It imports resolved prototype data from Factorio, normalizes it into reusable
dataset profiles, and calculates the production chain required for a target rate.

The project is under active development. The calculation engine, data importer,
profile storage, plan-file format, and initial Ratatui interface are implemented.
Some workflows are currently available only through command-line startup options or
the library API; see [Current limitations](#current-limitations).

## Current features

- Imports `data.raw` JSON produced by Factorio's `--dump-data` workflow.
- Optionally applies item, fluid, recipe, and entity names from prototype-locale
  JSON dumps.
- Stores normalized data as named, fingerprinted dataset profiles.
- Supports items, fluids, recipes, assembling machines, furnaces, modules, fuels,
  burner machines, and transport belts represented by supported prototype fields.
- Expands recipe dependencies and combines demand for shared intermediates across
  multiple targets.
- Selects deterministic default recipes, machines, and fuels, with explicit
  overrides supported by the planner and workspace.
- Calculates expected output for probabilistic and ranged products.
- Applies module speed, productivity, and energy-consumption effects while
  validating machine and recipe restrictions.
- Reports fractional and installed machine counts, electric demand, burner-fuel
  demand, external inputs, co-product surplus, item/fluid flows, and optional belt
  equivalents.
- Detects dependency and burner-fuel cycles and reports their complete paths.
- Displays an aggregated production table and a per-target dependency tree.
- Reads and writes versioned `*.fptplan.json` plan files with dataset fingerprint
  validation and explicit rebinding support in the application layer.

The planner is independent of terminal and filesystem code, so calculations can be
used and tested without starting the TUI.

## Requirements

- A Rust toolchain with Rust 2024 edition support
- An interactive terminal at least 60 columns by 12 rows
- A Factorio `data.raw` JSON dump for real game data

## Build and test

```sh
cargo build
cargo test
```

Run Clippy and formatting checks with:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features
```

## Import a dataset

A named import is performed before the TUI opens:

```sh
cargo run -- \
  --import-data /path/to/data.raw.json \
  --profile vanilla
```

An optional locale directory may contain any of these recognized files:

- `item-locale.json`
- `fluid-locale.json`
- `recipe-locale.json`
- `entity-locale.json`

```sh
cargo run -- \
  --import-data /path/to/data.raw.json \
  --locale /path/to/prototype-locale \
  --profile vanilla-en
```

Profile names must be non-empty and cannot contain path separators, control
characters, `.` or `..`. Importing over an existing profile name is rejected.

Dataset profiles and logs are stored in the platform application-data directory
resolved by the `directories` crate for `factorio-planner-tui`. On Linux this is
normally below `$XDG_DATA_HOME/factorio-planner-tui`, or
`~/.local/share/factorio-planner-tui` when `XDG_DATA_HOME` is unset.

## Run

Open the start screen and use the active profile:

```sh
cargo run
```

Open a particular cached dataset:

```sh
cargo run -- --dataset vanilla
```

Open an existing plan:

```sh
cargo run -- --plan /path/to/base.fptplan.json
```

Use `cargo run -- --help` for the complete startup option list. `--locale` and
`--profile` require `--import-data`; `--plan`, `--dataset`, and `--import-data`
represent mutually exclusive startup paths.

## TUI controls

| Key | Action |
| --- | --- |
| `j` / `k`, arrow keys | Move the current selection |
| `Tab` / `Shift+Tab` | Move focus between panes |
| `Enter` | Activate or confirm |
| `Esc` | Close or cancel the current overlay |
| `t` | Toggle aggregated table/dependency tree |
| `r` | Select a recipe for the selected production step |
| `m` | Select a machine |
| `u` | Configure modules |
| `f` | Select burner fuel |
| `b` | Select the belt used for throughput equivalents |
| `x` | Toggle the selected commodity as an external input |
| `?` | Open help |
| `q` or `Ctrl+C` | Quit; dirty plans require confirmation |

Selection overlays can be filtered by typing. Use Backspace to edit the query.

## Calculation model

All internal rates use units per second. Results preserve fractional values and
round only for display or installed machine/belt counts. The calculator chooses
defaults deterministically, but it does not optimize a factory or choose a preferred
resource strategy for the user.

Commodities with no supported production recipe are treated as external inputs.
They can also be marked external explicitly to stop recursive expansion. Unsupported
prototype mechanics generate import diagnostics rather than being silently applied.

## Current limitations

The initial release model intentionally does not simulate physical factory layout,
resource patches, mining drills, offshore pumps, beacons, quality, spoilage,
recycling, heat- or fluid-powered machines, pipe capacity, inserter throughput,
trains, bots, technology unlocks, or automatic recipe optimization.

The current TUI also has these workflow gaps:

- The start-screen **Import data** action only directs the user to the
  `--import-data` CLI workflow.
- The start-screen **Open plan** action only directs the user to the `--plan` CLI
  workflow.
- Plan persistence exists in the application/library layer, but no save command is
  currently bound in the interactive TUI.
- Creating a plan interactively starts with one target. Additional target editing
  actions exist in application state but are not yet bound to keys.

See [`plans/implementation.md`](plans/implementation.md) for the staged roadmap and
the complete intended first-release scope.

## Project structure

```text
src/
  app.rs          application state, actions, and screen transitions
  catalog.rs      normalized Factorio data and typed identifiers
  cli.rs          command-line parsing and startup modes
  import.rs       data.raw and prototype-locale parsing
  persistence.rs  dataset profiles and versioned plan files
  planner.rs      pure production-rate calculations
  tui.rs          terminal lifecycle, input translation, and rendering
  main.rs         executable entry point
```

Integration tests under `tests/` cover import normalization, localization,
catalog behavior, planner calculations, persistence, application actions, CLI
startup, and TUI rendering/input translation.

## Development

Development follows the stages in [`plans/implementation.md`](plans/implementation.md)
and uses test-driven development:

1. Add a failing test for the desired behavior or bug.
2. Implement the smallest change that passes it.
3. Refactor while keeping the complete suite green.

Keep factory-planning domain logic independent from terminal rendering and input
handling. Run `cargo test` before considering a change complete.
