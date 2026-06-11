# Factorio Planner TUI Implementation Plan

## 1. Purpose

Build a keyboard-first terminal application for calculating Factorio factory
requirements. The application will import the resolved prototype data produced
by a user's Factorio installation, support modded recipe sets, and calculate the
machines, inputs, outputs, power, fuel, and transport capacity required to meet
one or more production targets.

The first release is a deterministic rate calculator, not a factory-layout
simulator or an optimization solver. Users remain responsible for selecting
ambiguous recipes, machines, modules, and fuels.

All development must follow test-driven development:

1. Add a failing test that specifies the next behavior.
2. Implement only enough production code to make the test pass.
3. Refactor while keeping the complete test suite green.

## 2. First-Release Capabilities

The first usable release must:

- Import `data.raw` JSON produced by Factorio's `--dump-data` command.
- Optionally import names produced by `--dump-prototype-locale`.
- Cache imported data as named, normalized dataset profiles.
- Support arbitrary modded items, fluids, recipes, crafting machines, modules,
  fuels, and transport belts when they use supported prototype fields.
- Allow multiple item or fluid targets in one factory plan.
- Combine demand for shared intermediates.
- Allow explicit recipe, machine, module, fuel, and external-input choices.
- Calculate expected rates for probabilistic products.
- Calculate fractional and rounded-up machine counts.
- Calculate machine-module speed, productivity, and energy-consumption effects.
- Report electric demand and burner-fuel consumption.
- Report exact and rounded-up belt equivalents for item flows.
- Report fluid flow rates without claiming a pipe count.
- Report raw/external inputs and secondary-product surplus.
- Save and reopen versioned factory-plan files.
- Present both an aggregated production table and a dependency-tree view.
- Provide both TUI workflows and optional command-line paths for imports,
  dataset selection, and plan opening.

The first release will not model:

- Resource patches, mining drills, offshore pumps, or other extraction.
- Beacon placement or beacon effects.
- Quality, spoilage, freshness, or recycling behavior.
- Heat- or fluid-powered machine calculations.
- Pipe-network capacity or fluid-network simulation.
- Inserter throughput.
- Belt routing, lane balancing, train logistics, bots, or physical layout.
- Technology progression or recipe unlock state.
- Automatic recipe optimization or resource minimization.

Unsupported mechanics must generate visible diagnostics. They must never be
silently ignored when doing so could make a calculation incorrect.

## 3. Technical Direction

### 3.1 Crate structure

Keep the executable entry point small and put application behavior in a library
crate. Organize the code by responsibility:

```text
src/
  main.rs
  lib.rs
  cli.rs
  app/
  catalog/
  import/
  planner/
  persistence/
  tui/
```

The boundaries are:

- `catalog`: normalized Factorio data and stable typed identifiers.
- `import`: raw dump parsing, normalization, locale loading, and diagnostics.
- `planner`: pure rate calculations with no terminal or filesystem access.
- `persistence`: dataset profiles, plan files, versioning, and atomic writes.
- `app`: application state, actions, commands, and screen transitions.
- `tui`: terminal lifecycle, event translation, and pure Ratatui rendering.
- `cli`: command-line parsing and startup-mode selection.

The planner must accept catalog and plan values and return a result or structured
diagnostics. It must not read files, inspect the terminal, or mutate application
state.

### 3.2 Initial dependencies

Use Rust crates rather than external services or non-Rust runtime components.
Add dependencies only when their phase begins.

Expected runtime dependencies:

- `ratatui` for terminal rendering.
- `crossterm` for terminal setup and input events.
- `clap` for command-line parsing.
- `serde` and `serde_json` for import and persistence formats.
- `thiserror` for library error types.
- `directories` for platform application-data paths.
- `blake3` for dataset fingerprints.
- `indexmap` for deterministic iteration and serialized output.
- `tracing` and `tracing-subscriber` for file-based diagnostics.
- `unicode-width` if Ratatui does not cover all required text measurements.

Expected development dependencies:

- `pretty_assertions` for readable structural assertions.
- `proptest` for planner invariants.
- `insta` for selected TUI snapshots.
- `tempfile` for persistence tests.
- `assert_cmd` and `predicates` for CLI integration tests.

Do not introduce asynchronous execution until profiling demonstrates a need.
Import can initially run synchronously behind a progress/status screen.

### 3.3 Numeric conventions

- Normalize all rates to units per second.
- Normalize energy to joules and power to watts.
- Store prototype quantities and calculated rates as validated `f64` values.
- Reject non-finite values and invalid negative values at import or plan-input
  boundaries.
- Keep fractional values internally; round only for display.
- Show both fractional machines and installed machines rounded up to an integer.
- Make display rates switchable between per-second, per-minute, and per-hour.

Floating point is appropriate because Factorio prototype data and module effects
are floating point. Tests should use tolerances for calculated values and exact
comparisons for identifiers, choices, and graph structure.

## 4. Core Data Contracts

### 4.1 Typed identifiers

Use newtypes rather than interchangeable strings for:

- Item IDs.
- Fluid IDs.
- Commodity IDs, represented as either item or fluid.
- Recipe IDs.
- Machine IDs.
- Module IDs.
- Fuel IDs.
- Belt IDs.
- Dataset fingerprints.

Internal prototype names are authoritative stable IDs. Localized names are
display metadata only and must never be serialized as references.

### 4.2 Normalized catalog

The importer should produce a compact `Catalog` containing:

- Commodities and optional localized display names.
- Recipes with category, craft duration, ingredients, products, main product,
  visibility, productivity policy, and supported/unsupported flags.
- Crafting machines with categories, crafting speed, module slots, allowed
  effects/categories, energy usage, and energy-source information.
- Modules with category and supported speed, productivity, and consumption
  effects.
- Fuel items with category, fuel value, and optional burnt result.
- Transport belts with full-belt item throughput.
- Reverse indexes from products to recipes and categories to machines.
- Import metadata and warnings.

Do not retain graphics, sounds, collision boxes, animations, or unrelated
prototype data in the normalized profile.

### 4.3 Factory plan

A versioned plan file should contain:

- Schema version.
- Plan name.
- Bound dataset profile name and fingerprint.
- One or more targets, each with commodity and positive rate per second.
- Recipe choice per planned commodity.
- Machine choice per selected recipe.
- Installed module choices per selected production step.
- Burner-fuel choice per burner production step.
- Set of commodities treated as external inputs.
- Selected belt used for item-logistics equivalents.
- Preferred display-rate unit.

Derived calculation results must not be persisted as authoritative data. Rebuild
them from the plan and bound catalog whenever the plan changes or opens.

Use `.fptplan.json` as the plan filename suffix.

### 4.4 Calculation result

Return a result containing:

- Aggregated production steps.
- A dependency tree per target.
- External item and fluid inputs.
- Item and fluid flow totals.
- Co-product surplus.
- Electric demand.
- Burner-fuel demand and burnt-result surplus.
- Exact and rounded belt equivalents.
- Non-fatal warnings.

Each production step should expose its selected recipe, planning product,
required output rate, craft rate, machine configuration, fractional machine
count, installed machine count, ingredients, products, and power/fuel values.

## 5. Calculation Rules

### 5.1 Recipe defaults and overrides

When a commodity has multiple recipes, choose a deterministic default:

1. Prefer visible recipes whose explicit `main_product` matches the commodity.
2. Then prefer visible recipes with the commodity as their only product.
3. Break remaining ties by recipe ID in lexical order.

The user may override the selected recipe. Hidden recipes remain available only
when explicitly selected or when no visible supported recipe exists.

### 5.2 External boundaries

A commodity is external when:

- The user explicitly marks it external.
- No supported recipe produces it.
- Producing it requires unsupported behavior.

External demand is accumulated and displayed rather than recursively expanded.
Extraction is therefore represented as an external input in the first release.

### 5.3 Shared intermediates

Aggregate the ingredient demand generated by all targets and downstream steps.
A commodity produced once for several consumers must appear as one production
step in the table, with its total required rate.

Recalculation should iterate until the demand totals and selected production
steps stabilize. The output must not depend on target insertion order.

### 5.4 Cycles

Detect cycles in the selected recipe dependency graph before calculating rates.
Return the complete cycle path as a structured diagnostic. Do not guess a cycle
break. The user must choose another recipe or mark at least one commodity in the
cycle as external.

### 5.5 Probabilistic products

Use expected output:

```text
expected_amount = probability * expected(amount)
```

For an amount range, use the arithmetic mean of the minimum and maximum. A fixed
amount is its own expected amount. Reject products whose expected planning
output is zero.

The result represents long-run steady-state rates and should say so in the UI.

### 5.6 Multi-product recipes

Each selected recipe has one planning product: the commodity whose demand caused
the recipe to be selected. Size the recipe from that product only.

All other products are reported as surplus. Do not automatically credit surplus
against other demand, because that would require a balancing solver and could
make results dependent on allocation order.

### 5.7 Machine selection

A machine is compatible when it supports the recipe category and the recipe
does not require an unsupported machine behavior.

Default to the compatible machine with the greatest effective base crafting
speed. Break ties by machine ID. Users may choose any compatible machine.

Calculate:

```text
crafts_per_second_per_machine =
    effective_crafting_speed / recipe_energy_required

required_machines =
    required_crafts_per_second / crafts_per_second_per_machine
```

Use Factorio's documented defaults when an optional supported field is absent.
Record the defaulting decision in importer tests.

### 5.8 Machine modules

Support modules inserted into the selected crafting machine. Do not support
beacons in the first release.

Validate:

- Module count does not exceed machine slots.
- Machine allows the module category and effect types.
- Recipe allows the module category and effect types.
- Productivity is allowed for the recipe.

Apply supported module speed, productivity, and consumption effects according
to Factorio's stacking rules. Enforce recipe maximum-productivity limits and
honor products excluded from productivity where represented by the dump.

Unsupported module effects should be retained as warnings. A module with an
unsupported effect may not be selected until support exists, because partially
applying a module could misrepresent the plan.

### 5.9 Power

For electric machines, calculate:

- Average active power from energy usage and supported consumption effects.
- Fractional-process demand using fractional machine count.
- Installed full-load demand using rounded-up machine count.

Display both fractional-process and installed full-load values so users can
distinguish steady-state demand from worst-case simultaneous operation.

Include machine drain only if it can be normalized accurately from the imported
prototype. Otherwise issue a visible limitation warning rather than inventing a
value.

### 5.10 Burner fuel

For burner machines:

- Filter fuel items by accepted fuel category.
- Select a deterministic default by greatest fuel value, then lexical ID.
- Allow the user to choose another compatible fuel.
- Calculate fuel rate from machine energy usage, energy-source effectivity, and
  fuel value.
- Add fuel demand to the recursive plan unless that fuel is external.
- Report burnt results as surplus.

Flag heat, fluid, and unknown machine energy sources as unsupported for power
and fuel calculations in the first release.

### 5.11 Item logistics

For each imported transport belt:

```text
full_belt_items_per_second = prototype_speed * 480
```

For the plan's selected belt, report:

```text
exact_belts = item_rate / full_belt_items_per_second
installed_belts = ceil(exact_belts)
```

This is a capacity equivalent, not a routed belt design. Do not estimate belt
length, splitters, underground belts, lane allocation, or inserters.

### 5.12 Fluid logistics

Display fluid rates in the selected time unit. Do not convert rates to pipe
counts because Factorio pipe throughput depends on the network topology, pumps,
segment lengths, and junctions.

## 6. Import And Dataset Profiles

### 6.1 User export workflow

Document the official commands:

```text
factorio --dump-data
factorio --dump-prototype-locale
```

The application imports files already produced by Factorio. It does not launch
Factorio in the first release.

### 6.2 Import pipeline

Implement import as explicit stages:

1. Validate that the source file exists and can be read.
2. Compute a source fingerprint while reading it.
3. Parse only relevant top-level prototype collections.
4. Convert raw fields into intermediate import structures.
5. Normalize energy strings, amounts, probabilities, categories, and defaults.
6. Build typed catalog records and reverse indexes.
7. Validate references and supported behavior.
8. Parse optional locale output and attach display names.
9. Collect warnings and rejected-prototype diagnostics.
10. Serialize the normalized profile atomically.

The dump may be large. Avoid deserializing unrelated prototype collections into
strongly typed structures. Start with a streaming top-level visitor; benchmark
before introducing additional complexity.

### 6.3 Import diagnostics

Every diagnostic should include:

- Severity: warning or error.
- Prototype type and ID when known.
- JSON field path when known.
- Short explanation.
- Whether the prototype was retained, partially retained, or rejected.

Fatal errors prevent profile creation. Warnings allow creation but remain
visible from the profile and planning screens.

Unknown fields are forward-compatible and ignored. Unknown shapes for required
supported fields are errors for that prototype, not panics.

### 6.4 Profile storage

Use the platform application-data directory through `directories`. Store:

- A profile index.
- One normalized, versioned catalog file per profile.
- Import metadata and fingerprints.
- Application logs.

Profile operations:

- List profiles.
- Import a new named profile.
- Replace an existing profile after confirmation.
- Select the active profile.
- Delete an unused profile after confirmation.
- Display source paths, import time, fingerprint, and warning count.

Fingerprint the source dump, optional locale dump, and importer schema version
with BLAKE3. Plans bind to the resulting dataset fingerprint.

## 7. Persistence And Compatibility

### 7.1 Versioning

Give normalized profile files and plan files independent integer schema
versions. Parsing must first inspect the version and then dispatch to the
corresponding representation.

For the first release:

- Read the current version.
- Return a clear unsupported-version error for newer files.
- Add explicit migrations when a later release changes a schema.

### 7.2 Atomic writes

Write profiles and plans by:

1. Serializing to a temporary file in the destination directory.
2. Flushing and syncing the temporary file.
3. Renaming it over the target.

Keep the existing destination intact when serialization or writing fails.

### 7.3 Dataset mismatch

When opening a plan:

- Use the exact named profile when its fingerprint matches.
- Search other profiles for the exact fingerprint if the name is absent.
- If no exact fingerprint exists, open plan metadata in a blocked state.
- Show missing commodity, recipe, machine, module, fuel, and belt references
  before rebinding.
- Require an explicit user action to bind the plan to a different dataset.
- Recalculate only after all required references are valid or reset.

Never silently rebind a plan based only on profile name.

### 7.4 Dirty state

Mark a plan dirty after any persisted field changes. Saving clears the flag.
Opening another plan, replacing a dataset, or exiting with dirty state requires
save, discard, or cancel confirmation.

## 8. TUI Design

### 8.1 Terminal lifecycle

Implement a terminal guard that:

- Enables raw mode.
- Enters the alternate screen.
- Enables required input reporting.
- Restores all terminal state on normal exit.
- Restores terminal state during panic unwinding before reporting the panic.

Keep terminal events separate from application actions. Translate key events
into a small `Action` enum before mutating state.

### 8.2 Screens

Build these screens in order:

1. **Start screen:** choose a profile, import data, create a plan, or open a
   plan.
2. **Profile screen:** list profile metadata, warnings, replacement, and
   deletion actions.
3. **Planning workspace:** edit targets and inspect calculated results.
4. **Selection overlay:** searchable recipe, machine, module, fuel, belt, item,
   or fluid selection.
5. **Diagnostics overlay:** import and calculation warnings/errors.
6. **Help overlay:** context-sensitive key bindings and first-release limits.

### 8.3 Planning workspace layout

Use a responsive layout:

- Header: plan name, dataset profile, fingerprint status, and dirty marker.
- Left pane: targets and external-input controls.
- Main pane: aggregated table or dependency tree.
- Right or bottom pane: selected production-step configuration and details.
- Footer: context-sensitive key hints and current status.

On narrow terminals, replace side-by-side panes with tabs. If the terminal is
too small for the minimum layout, render a stable message with the required and
current dimensions.

### 8.4 Aggregated table

Include columns, subject to terminal width, for:

- Commodity/planning product.
- Required rate.
- Recipe.
- Machine.
- Fractional and installed machine counts.
- Modules.
- Electric power or fuel rate.
- Belt equivalent for item flows.
- Warning indicator.

Allow sorting and filtering without changing the calculation.

### 8.5 Dependency tree

Show one root per target. Each node displays:

- Commodity and required rate.
- Selected recipe and machine.
- Required machines.
- External-boundary or surplus status.

Shared intermediates may appear under multiple roots for traceability, while
the displayed aggregate total must come from the shared production step. Label
such nodes as shared to avoid implying duplicated construction.

### 8.6 Default key bindings

Use:

- Arrow keys or `j`/`k`: move selection.
- `Tab` and `Shift+Tab`: move focus.
- `Enter`: open or confirm.
- `Esc`: close overlay or cancel.
- `/`: search/filter.
- `a`: add target.
- `e`: edit selected target.
- `d`: delete selected target.
- `r`: choose recipe.
- `m`: choose machine.
- `u`: configure modules.
- `f`: choose burner fuel.
- `x`: toggle external-input boundary.
- `b`: choose belt.
- `t`: switch table/tree view.
- `s`: save.
- `?`: open help.
- `q`: request exit.

Do not bind destructive actions without confirmation when data would be lost.

## 9. CLI Contract

Provide these options:

```text
factorio_planner_tui [OPTIONS]

--import-data <PATH>    Import a Factorio data.raw dump
--locale <PATH>         Optional locale dump used with --import-data
--profile <NAME>        Name used for import or profile selection
--dataset <NAME>        Open the TUI with an existing dataset profile
--plan <PATH>           Open an existing factory plan
```

Rules:

- `--locale` requires `--import-data`.
- `--profile` names a new/replaced import when `--import-data` is present.
- `--dataset` selects an existing profile.
- `--plan` may be used alone because the plan records its dataset binding.
- Conflicting dataset selections are rejected with actionable messages.
- Without arguments, launch the start screen.
- Import failures return a non-zero process status.

The CLI may perform import before entering the TUI, but should still show a
summary and require confirmation before replacing an existing profile unless a
future explicit non-interactive flag is added.

## 10. Step-By-Step Delivery

Each step below is a separate TDD milestone. Keep commits small enough that the
tests and behavioral change can be reviewed together.

### Step 1: Establish the quality baseline

1. Convert the package into a binary plus library crate.
2. Add module placeholders only as needed by the first tests.
3. Add project-wide formatting and Clippy expectations.
4. Add a CI workflow running:
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test --all-targets --all-features`
5. Add a `tests/fixtures` convention for small hand-authored dumps.
6. Verify that the initial smoke test invokes the library successfully.

Exit criteria: formatting, Clippy, and tests pass from a clean checkout.

### Step 2: Build validated domain primitives

1. Write tests for typed IDs and commodity item/fluid distinction.
2. Add validated positive/non-negative numeric wrappers where they prevent
   invalid domain state.
3. Test recipe, product, ingredient, machine, module, fuel, and belt records.
4. Add deterministic catalog indexing and lookup.
5. Test duplicate IDs and broken references as structured errors.

Exit criteria: a catalog can be assembled entirely in memory and rejects invalid
state without any TUI or JSON code.

### Step 3: Parse minimal recipe data

1. Create the smallest Factorio-like dump fixture containing items, fluids, and
   recipes.
2. Write a failing import test for fixed item inputs and outputs.
3. Add support for fluid ingredients and products.
4. Add recipe category, craft duration, visibility, and main-product defaults.
5. Add diagnostics for malformed supported fields.

Exit criteria: the normalized catalog contains valid commodities and recipes
from a fixture and reports malformed records precisely.

### Step 4: Add products and expected-value math

1. Test fixed amounts.
2. Test minimum/maximum ranges.
3. Test probability.
4. Test combined range and probability.
5. Test duplicate product entries and expected-value aggregation.
6. Test zero expected output rejection for a planning product.

Exit criteria: expected recipe products match hand-calculated values.

### Step 5: Import crafting machines

1. Add fixtures for assemblers, furnaces, and modded crafting categories.
2. Test crafting-category compatibility.
3. Test crafting speed and recipe-duration calculations.
4. Normalize module slots and supported effect restrictions.
5. Normalize electric and burner energy-source metadata.
6. Warn on unsupported energy sources.

Exit criteria: every supported recipe can list deterministic compatible
machines.

### Step 6: Import modules, fuels, and belts

1. Test module categories and speed/productivity/consumption effects.
2. Test fuel categories, fuel values, and burnt results.
3. Test transport-belt throughput using `speed * 480`.
4. Reject invalid energy strings and non-positive belt throughput.
5. Preserve unsupported module effects as selection-blocking warnings.

Exit criteria: the catalog exposes all data required by machine configuration,
burner calculations, and belt equivalents.

### Step 7: Add optional localization

1. Capture representative locale-dump fixtures.
2. Test localized names for commodities, recipes, machines, modules, fuels, and
   belts.
3. Test fallback to internal IDs.
4. Test searching by both localized name and internal ID.
5. Ensure localized names never replace serialized IDs.

Exit criteria: localized display is optional and cannot break plan references.

### Step 8: Persist named dataset profiles

1. Define the normalized profile schema and version.
2. Test profile-path resolution with a temporary application-data root.
3. Test create, list, select, replace, and delete.
4. Test BLAKE3 fingerprints for data, locale, and importer schema.
5. Test atomic writes and preservation of an existing profile after failure.
6. Test import warning summaries.

Exit criteria: a dump can be imported once and reopened from a compact cached
profile without reparsing the raw dump.

### Step 9: Calculate a single deterministic chain

1. Test one target produced by one recipe and machine.
2. Test recursive ingredient expansion.
3. Test external inputs.
4. Test fractional and rounded machine counts.
5. Test per-second base units and display-unit conversion.
6. Keep the planner API pure.

Exit criteria: a simple chain produces correct machines and external inputs.

### Step 10: Add multiple targets and shared intermediates

1. Test two targets with a shared intermediate.
2. Test duplicate targets being summed.
3. Test target-order independence.
4. Test removal and rate edits.
5. Add property tests for linear scaling and non-negative finite results.

Exit criteria: combined plans aggregate shared demand exactly once.

### Step 11: Add explicit recipe choices and cycles

1. Test deterministic default recipe ordering.
2. Test recipe override.
3. Test missing and unsupported recipe handling.
4. Test direct and indirect cycle detection.
5. Test resolving a cycle by selecting another recipe.
6. Test resolving a cycle with an external boundary.

Exit criteria: cycles cannot produce a misleading partial result and diagnostics
show the full selected-recipe cycle.

### Step 12: Add multi-product surplus

1. Test a recipe with one planning product and one secondary product.
2. Size the recipe from the planning product only.
3. Report expected secondary output as surplus.
4. Test that surplus does not reduce another target or ingredient demand.
5. Test that choosing another planning product changes sizing predictably.

Exit criteria: co-products are transparent without introducing hidden
optimization.

### Step 13: Add machine and module configuration

1. Test fastest-compatible-machine defaults and lexical tie-breaking.
2. Test explicit machine overrides.
3. Test module slot limits and restrictions.
4. Test speed and productivity effects.
5. Test consumption effects and productivity caps.
6. Test products excluded from productivity.
7. Test invalid saved configurations after a dataset change.

Exit criteria: machine counts and product rates reflect only valid selected
modules.

### Step 14: Add power and burner fuel

1. Test electric fractional-process power.
2. Test installed full-load power.
3. Test module consumption effects.
4. Test burner effectivity and compatible fuel selection.
5. Test recursive fuel demand.
6. Test burnt-result surplus.
7. Test burner-fuel dependency cycles.
8. Test unsupported heat/fluid source diagnostics.

Exit criteria: supported machines report actionable power or fuel requirements.

### Step 15: Add logistics summaries

1. Test exact and rounded belt equivalents for each item rate.
2. Test belt selection and no-belt state.
3. Test multiple item flows sharing the same selected belt type.
4. Test that fluid flows show rates but no pipe counts.
5. Label belt values as capacity equivalents.

Exit criteria: users can translate item rates into belt capacity without the
application implying a routed design.

### Step 16: Persist factory plans

1. Define and test the versioned `.fptplan.json` schema.
2. Test save/load round trips.
3. Test atomic writes.
4. Test dirty-state transitions.
5. Test exact dataset-fingerprint binding.
6. Test missing-profile fingerprint lookup.
7. Test blocked mismatch state and explicit rebinding.
8. Test missing references after rebinding.

Exit criteria: plans reopen reproducibly or fail safely with actionable
diagnostics.

### Step 17: Build the application state machine

1. Define screens, focus targets, overlays, and application actions.
2. Test startup-mode selection from CLI inputs.
3. Test action handling without a terminal.
4. Test target and production-step editing transitions.
5. Test dirty-exit confirmation.
6. Test error and warning presentation state.

Exit criteria: complete workflows can be driven in tests through actions before
rendering begins.

### Step 18: Add terminal lifecycle and events

1. Add the Ratatui and Crossterm terminal guard.
2. Test event-to-action translation.
3. Handle press events, resize events, and clean shutdown.
4. Add panic-safe terminal restoration.
5. Add file logging outside the alternate screen.

Exit criteria: normal exits and controlled failures always restore the terminal.

### Step 19: Render startup and profile workflows

1. Snapshot the empty start screen.
2. Render profile selection and metadata.
3. Add import path and profile-name prompts.
4. Render progress, success summaries, warnings, and fatal errors.
5. Add profile replace/delete confirmations.
6. Test narrow-terminal variants.

Exit criteria: users can establish a dataset without command-line arguments.

### Step 20: Render the planning workspace

1. Render target editing.
2. Render the aggregated table.
3. Render the dependency tree.
4. Render the selected-step details/configuration pane.
5. Add searchable selectors for all explicit choices.
6. Add diagnostics and help overlays.
7. Add responsive tabbed behavior for narrow terminals.
8. Snapshot critical states with `TestBackend`.

Exit criteria: every planner choice and diagnostic is available through the
keyboard-only TUI.

### Step 21: Complete CLI integration

1. Test all documented arguments and validation rules.
2. Import data before TUI startup when requested.
3. Open plans and datasets directly.
4. Return non-zero statuses for argument and import failures.
5. Add CLI smoke tests with temporary profile directories.

Exit criteria: scripted startup paths and interactive startup reach the same
application states.

### Step 22: Harden and document

1. Benchmark representative large prototype dumps.
2. Remove import copies or allocations shown to be material by profiling.
3. Test Unicode names and terminal-width calculations.
4. Test malformed/truncated JSON and interrupted writes.
5. Test all supported calculations against independent hand-worked fixtures.
6. Write user documentation for export, import, planning, keys, persistence,
   supported mechanics, and known limitations.
7. Run formatting, Clippy, unit, property, snapshot, integration, and smoke
   tests.

Exit criteria: the release acceptance workflow passes and all limitations are
clearly documented.

## 11. Test Strategy

### 11.1 Unit tests

Cover:

- Validation and typed IDs.
- Import defaults and normalization.
- Energy-string parsing.
- Expected probabilistic outputs.
- Recipe and machine compatibility.
- Recipe defaults and overrides.
- Cycle paths.
- Shared intermediates.
- Module effects and restrictions.
- Productivity caps/exclusions.
- Machine counts.
- Electric and burner calculations.
- Co-product and burnt-result surplus.
- Belt throughput and display-unit conversion.
- Version parsing and migrations.

### 11.2 Property tests

Assert:

- Valid calculated rates are finite and non-negative.
- Multiplying every target by a positive factor multiplies all unconstrained
  rates and fractional machine counts by that factor.
- Reordering targets does not change aggregate results.
- Combining duplicate targets equals one target with the summed rate.
- Unit conversion does not alter base per-second values.

### 11.3 Import fixtures

Keep fixtures small and hand-authored. Include:

- A basic deterministic item chain.
- Fluids and custom crafting categories.
- Probabilistic and ranged products.
- Multiple products.
- Alternate recipes.
- A dependency cycle.
- Electric and burner machines.
- Valid and invalid modules.
- Modded fuels and belts.
- Unknown fields and unsupported mechanics.
- Malformed required fields.
- Locale output with missing entries.

Do not commit a full proprietary game-data dump.

### 11.4 TUI tests

Use Ratatui's `TestBackend` and snapshots for:

- Start screen with and without profiles.
- Import success, warning, and error states.
- Aggregated table.
- Dependency tree.
- Target editor.
- Recipe/machine/module/fuel/belt selectors.
- Dataset mismatch.
- Help and diagnostics overlays.
- Dirty-exit confirmation.
- Minimum and narrow terminal layouts.

Test action handling separately from rendering so snapshots do not become the
only behavioral coverage.

### 11.5 Integration tests

Cover:

- CLI argument precedence and validation.
- Profile import and replacement.
- Plan save/open round trips.
- Atomic-write failure behavior.
- Dataset fingerprint lookup and mismatch.
- Missing catalog references.
- Process exit status.
- Terminal startup/shutdown through the narrowest practical smoke test.

## 12. Release Acceptance Scenario

The first release is complete when a user can:

1. Run Factorio's `--dump-data` command for a modded installation.
2. Launch the application and import the resulting JSON as a named profile.
3. Optionally attach localized prototype names.
4. Create a plan with multiple item and fluid output targets.
5. Review and change selected recipes, machines, modules, fuels, external
   boundaries, and belt type.
6. Resolve any selected-recipe cycles through explicit choices.
7. Inspect combined machine counts, inputs, surplus, electric power, burner
   fuel, item belt equivalents, and fluid rates.
8. Switch between aggregated table and dependency-tree views.
9. Save the plan, exit cleanly, and reopen it with identical results.
10. Receive a blocked, actionable mismatch screen instead of incorrect results
    when the required dataset is unavailable or changed.

Before release, all of these commands must pass:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## 13. External References

- Factorio command-line export options:
  <https://wiki.factorio.com/Command_line_parameters>
- Factorio recipe prototype:
  <https://lua-api.factorio.com/latest/prototypes/RecipePrototype.html>
- Factorio assembling-machine prototype:
  <https://lua-api.factorio.com/latest/prototypes/AssemblingMachinePrototype.html>
- Factorio module prototype:
  <https://lua-api.factorio.com/latest/prototypes/ModulePrototype.html>
- Factorio energy-source union:
  <https://lua-api.factorio.com/latest/types/EnergySource.html>
- Factorio transport-belt prototype:
  <https://lua-api.factorio.com/latest/prototypes/TransportBeltPrototype.html>

These references define import behavior. The normalized catalog must isolate the
rest of the application from changes in Factorio's raw JSON representation.
