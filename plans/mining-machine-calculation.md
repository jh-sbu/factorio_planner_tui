# Mining Machine Calculation Plan

## 1. Purpose

Resource sources currently prevent ores and crude oil from appearing as
unexplained external inputs, but they stop at an abstract extraction rate. For
example, a plan that needs `9 iron-ore/s` can show `resource: iron-ore`, but it
does not translate that rate into electric mining drills, burner mining drills,
pumpjacks, or Space Age big mining drills.

This plan adds explicit machine-backed extraction so mined resources can report
selected miners, module effects, machine counts, energy demand, burner fuel
demand, and required mining fluids.

Development must follow project TDD:

1. Add a failing importer, catalog, planner, persistence, app, or TUI test for
   the next behavior.
2. Implement the smallest change needed to pass it.
3. Refactor while keeping `cargo test` green.

## 2. Scope

Add support for resource extraction machines:

- Base-game mining drills, including `burner-mining-drill` and
  `electric-mining-drill`.
- Pumpjacks for fluid resources such as `crude-oil`.
- Modded mining drills that use supported mining-drill prototype fields.
- Space Age big mining drills when their prototype fields fit the same model.

Do not model resource patch size, richness depletion over time, physical miner
placement, mining productivity technologies, quality, spoilage, or productivity
research in this phase. If any unsupported prototype field would make the
calculation misleading, import should produce a visible diagnostic instead of
silently accepting partial behavior.

## 3. Data Model Direction

Introduce a normalized mining-machine record in the catalog. It can either be a
new `MiningMachine` type or a generalized machine type shared with crafting
machines. Prefer the smallest shape that reuses existing module and energy
logic without forcing mining drills into recipe categories.

Minimum mining-machine data:

- Stable machine ID.
- Display name metadata through the existing localization path.
- Resource categories the machine can mine.
- Mining speed.
- Module slots.
- Allowed module effects and module categories.
- Energy source, energy usage, drain, burner effectivity, fuel categories, and
  burnt-result behavior where applicable.
- Support status and diagnostics for unsupported mining-drill mechanics.

The catalog should expose:

```rust
mining_machine(id) -> Option<&MiningMachine>
mining_machines_for_resource_category(category) -> &[MiningMachineId]
```

Keep resource-source data separate from mining-machine data. A resource source
describes what can be mined and at what mining time; a mining machine describes
which entity performs the extraction.

## 4. Plan Choices And Persistence

Add explicit miner selection to `FactoryPlan`. Key the selection by planning
commodity unless implementation proves source IDs are safer:

```rust
miner_choices: BTreeMap<CommodityId, MiningMachineId>
```

Expected behavior:

- A resource-backed extraction step chooses a deterministic default compatible
  miner when no explicit choice exists.
- Users can explicitly choose among compatible miners.
- Incompatible miner choices are rejected by calculation with a structured
  planner error.
- Persisted plans without miner choices remain valid and derive defaults from
  the bound catalog.

Persistence work:

- Add DTO support for miner choices.
- Keep older plan files compatible by defaulting missing miner choices to an
  empty map.
- Validate that persisted miner choices reference existing mining machines and
  compatible planned commodities.
- Add migration tests for old plan files.

## 5. Planner Calculation

Extend `ExtractionStep` from an abstract source row into an optional
machine-backed extraction row.

New extraction-step fields should include:

- Selected mining machine.
- Fractional machine count.
- Installed machine count.
- Installed modules.
- Speed multiplier.
- Productivity effect.
- Energy-consumption multiplier.
- Electric power or burner fuel demand.
- Existing source, required output rate, extraction rate, required fluids, and
  product rates.

For machine-backed resources, calculate the required mining operations and
machine count from resource mining time and miner speed.

Base formula:

```text
mining_operations_per_second_per_miner =
    mining_speed * speed_multiplier / resource_mining_time

effective_product_per_operation =
    resource_product_amount * (1 + module_productivity_effect)

required_operations_per_second =
    required_output_rate / effective_product_per_operation

fractional_miners =
    required_operations_per_second / mining_operations_per_second_per_miner
```

Required mining fluid should scale with mining operations, not final product
rate after productivity:

```text
required_fluid_rate =
    required_operations_per_second * resource_required_fluid_amount
```

This distinction needs explicit tests because productivity modules should not
incorrectly reduce sulfuric-acid demand for uranium mining.

Energy and fuel should follow the same conventions as production steps:

- Electric machines report fractional-process and installed full-load power.
- Burner machines add fuel demand recursively to the plan.
- Burnt results from burner miners contribute surplus through the existing fuel
  accounting path.

## 6. Module Support

Reuse the existing module validation and multiplier behavior where possible.
Mining modules should follow the machine's module slots, allowed effects, and
allowed module categories.

Required behavior:

- Speed modules increase operations per second and reduce required miner count.
- Productivity modules increase product output per operation and reduce
  required operations per second.
- Consumption effects change power or burner fuel demand.
- Unsupported module effects or categories are rejected consistently with
  production steps.

Avoid adding mining productivity research in this phase. It is a separate
global bonus and should be planned independently.

## 7. UI Behavior

Update the aggregated table so resource extraction rows show miner information
instead of treating the machine columns as unavailable.

Minimum table behavior:

- Source remains visible as `resource: iron-ore`, `resource: crude-oil`, or the
  localized source label.
- Machine column shows the selected miner.
- Machines column shows fractional and installed miner counts.
- Energy column shows electric or burner demand.
- Belt equivalents continue to apply to mined item products.

Update selected-step details for extraction rows:

- Show selected miner.
- Show modules.
- Show speed, productivity, and consumption multipliers.
- Show required mining fluids.
- Show products.
- Show power or fuel demand.

Selection overlays should let users choose a compatible miner for a selected
resource-backed extraction step.

## 8. Test Plan

Add focused tests in this order.

Catalog and import tests:

- Imports a base-game-style electric mining drill with resource categories,
  mining speed, energy usage, drain, and module slots.
- Imports a burner mining drill with burner fuel categories and effectivity.
- Indexes compatible miners by resource category.
- Rejects or warns on unsupported mining-drill fields that would affect rates.

Planner tests:

- `iron-ore 9/s` with electric mining drill calculates the expected fractional
  and installed miner count.
- Explicit miner selection changes the selected machine and machine count.
- Incompatible miner/resource-category choices fail with a structured error.
- Speed modules reduce required miner count.
- Productivity modules reduce required operations and miner count.
- Consumption modules increase electric power or burner fuel demand.
- Burner mining drill fuel demand expands recursively.
- Uranium mining fluid demand scales by mining operations, not productivity-
  boosted output.
- Existing resource-source plans without miner choices still calculate.

Persistence tests:

- Saves and reopens miner choices.
- Opens older plan files without miner choices.
- Rejects missing or incompatible persisted miner choices.

TUI/app tests:

- Aggregated extraction rows display miner, machine counts, and energy.
- Selected extraction details display miner, modules, required fluids, and
  products.
- Miner selection overlay lists compatible miners and updates the plan.

Run `cargo test` before considering each implementation slice complete.

## 9. Suggested Implementation Slices

### Slice 1: Import And Catalog Mining Machines

Add the normalized mining-machine type, importer support, category index, and
catalog tests. Do not change planner output yet.

### Slice 2: Default Miner Counts

Teach resource extraction to select a default compatible miner and calculate
fractional and installed miner counts without modules. Add planner tests for
electric miners and pumpjacks.

### Slice 3: Energy And Burner Fuel

Add electric power and burner fuel accounting for extraction steps. Reuse
existing step energy structures if practical.

### Slice 4: Explicit Miner Selection

Add `FactoryPlan` miner choices, persistence, app actions, and validation.

### Slice 5: Modules

Add module validation and speed, productivity, and consumption effects for
mining machines. Add the uranium required-fluid/productivity regression test in
this slice.

### Slice 6: TUI Polish

Update aggregated rows, selected-step details, and miner selection overlays.

## 10. Open Decisions

- Whether mining machines should reuse `MachineId` or get a dedicated
  `MiningMachineId`. A dedicated ID is clearer, but reusing `MachineId` may
  reduce UI and persistence duplication if the catalog already treats all
  entities as selectable machines.
- Whether miner choices should be keyed by `CommodityId` or
  `ResourceSourceId`. `CommodityId` matches existing module choices and UI
  selection, while `ResourceSourceId` may be more precise for resources with
  multiple products.
- Whether pumpjacks should be implemented in the same slice as mining drills or
  as a follow-up. They share resource categories and mining speed, but crude-oil
  depletion and infinite-resource behavior may require diagnostics.
- How to surface unsupported Space Age big-mining-drill mechanics if its dumped
  prototype includes fields outside the base mining-drill model.
