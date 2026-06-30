# Non-Recipe Production Source Plan

## 1. Purpose

The planner currently expands production chains through recipe products only.
That works for most craftable items and fluids, but it leaves several base-game
commodities as external inputs even when Factorio has a structured production
mechanic for them. The most visible gap is `space-science-pack`, which is
produced by launching a satellite rather than by a recipe.

This plan adds a small, general production-source layer so the calculator can
model the common non-recipe mechanics without turning each one into bespoke
planner logic.

Development must follow project TDD:

1. Add a failing importer, catalog, planner, or app test for the next source
   behavior.
2. Implement the smallest change needed to pass it.
3. Refactor while keeping `cargo test` green.

## 2. Scope

Add support for these production source kinds:

- Recipe sources: existing behavior, preserved as the default source kind.
- Resource sources: mined resources such as ores and crude oil.
- Fluid sources and conversions: offshore pump water and boiler or heat
  exchanger steam.
- Rocket launch sources: launch products such as satellite to space science.

Also record a design note for fuel byproducts such as
`uranium-fuel-cell -> depleted-uranium-fuel-cell`, but do not implement them in
this phase unless a later task explicitly requires power-system byproduct
accounting.

## 3. Data Model Direction

Introduce a normalized `ProductionSource` concept in the catalog. A source
represents a way to produce a planning commodity and provides enough information
for dependency expansion and rate calculation.

Suggested variants:

```rust
enum ProductionSource {
    Recipe(RecipeId),
    Resource(ResourceSourceId),
    FluidSource(FluidSourceId),
    RocketLaunch(RocketLaunchSourceId),
}
```

Keep detailed source records separate from this enum so the catalog can store
source-specific fields without bloating every production step.

Minimum source data:

- Stable source ID.
- Produced commodity and amount per operation or per second.
- Required input commodities.
- Machine or entity category when machine counts can be calculated.
- Operation duration or output rate when applicable.
- Support status and diagnostics for fields the app cannot model yet.

The catalog should expose a unified reverse index:

```rust
sources_for_product(commodity) -> &[ProductionSource]
```

Recipe lookup can remain available for recipe-specific UI workflows, but
planner expansion should use production sources.

## 4. Phase 1: Preserve Recipe Sources

First wrap existing recipe resolution behind the new source abstraction without
changing behavior.

Tests:

- Existing recipe-only plans still calculate the same production steps,
  external inputs, surplus, energy, and belt equivalents.
- Multiple recipe choices still select deterministic defaults.
- Explicit recipe choices still validate that the recipe exists, is supported,
  and produces the requested commodity.

Implementation notes:

- Convert recipe product indexing into source indexing while keeping recipe
  indexes for overlays and explicit recipe selection.
- Keep persisted plan compatibility. Existing `recipe_choices` should not need
  a migration for this phase.
- Continue treating commodities with no supported source as external inputs.

## 5. Phase 2: Resource Sources

Import resource prototypes as production sources for mined products.

Base-game examples:

- `iron-ore`
- `copper-ore`
- `coal`
- `stone`
- `uranium-ore`
- `crude-oil`

Required behavior:

- Resource products should be discoverable through `sources_for_product`.
- Resource sources should include mining time, products, product probability,
  resource category, and required mining fluid when present.
- `uranium-ore` must require sulfuric acid according to its `required_fluid`
  and `fluid_amount`.
- `crude-oil` should be modeled as an infinite resource source with fluid
  output, but the first implementation may report pumpjack counts only if the
  importer also models compatible mining drills.

Open decision:

- Decide whether first-release resource sources should calculate mining-machine
  counts or simply expand dependencies and classify the source as extraction.
  Machine counts require importing mining drills and resource categories.

Tests:

- A plan targeting an ore no longer appears as an unexplained external input
  when resource-source support is enabled.
- A production chain that needs `uranium-ore` includes sulfuric acid demand.
- Unsupported resource fields produce visible import diagnostics instead of
  silent partial calculations.

## 6. Phase 3: Fluid Sources And Steam

Add non-recipe fluid production for water and steam.

Required behavior:

- Offshore pumps should provide water as a fluid source.
- Boilers and heat exchangers should convert water to steam at their prototype
  target temperature.
- Coal liquefaction should be able to expand its steam dependency to a steam
  source instead of treating steam as an unexplained external input.

Implementation notes:

- Offshore-pump output is implied by the prototype and pumping speed, not by a
  recipe product list.
- Boiler and heat-exchanger steam output is entity behavior, not recipe
  behavior. Model it as a fluid conversion source with water input and energy
  input.
- Heat exchangers depend on heat, which the current app does not model. The
  first implementation can mark heat-exchanger steam as unsupported while
  supporting burner boiler steam.

Tests:

- A target or dependency for water resolves to an offshore-pump source.
- A coal-liquefaction chain can account for boiler-produced steam.
- Heat-based steam support is either calculated correctly or produces a clear
  diagnostic saying heat systems are unsupported.

## 7. Phase 4: Rocket Launch Sources

Import `rocket_launch_products` from item prototypes as production sources.

Base-game example:

- Launching `satellite` produces `1000 space-science-pack`.

Required behavior:

- `space-science-pack` should resolve through a rocket launch source.
- The launch source should require the launched item as an input.
- The launched item should then expand through normal sources, so satellite and
  its ingredient chain appear in the dependency tree.
- The calculator should also account for rocket-part demand per launch.

Implementation notes:

- The raw `rocket_launch_products` field lives on the launched item, not on a
  recipe.
- A complete space-science chain needs both the launched item and the rocket
  itself. In base game, rocket parts come from the `rocket-part` recipe and are
  assembled in the rocket silo.
- Model rocket launch as a source with:
  - Product: `space-science-pack`.
  - Product amount: `1000`.
  - Input: `satellite` at `1`.
  - Rocket requirement: `rocket-part` at the silo's required launch amount.
- If the required rocket-part count cannot be imported generically from the
  rocket-silo prototypes, add an explicit diagnostic rather than hard-coding a
  silent base-game assumption.

Tests:

- Creating a `space-science-pack 1/s` plan produces a non-empty chain.
- The chain includes satellite demand at `0.001/s` for 1000 science per launch.
- The chain includes rocket-part demand at the imported launch requirement.
- The dependency tree expands satellite ingredients and rocket-part
  ingredients through existing recipe behavior.
- The TUI no longer shows `space-science-pack 1/s` as a required external input
  when the catalog contains a supported rocket launch source.

## 8. Source Selection And UI

Recipe selection currently assumes all planned commodities choose recipes.
After source support, source selection needs to distinguish source kind.

Minimum UI behavior:

- Existing `r` recipe selection continues to work for recipe-backed steps.
- Non-recipe steps display a clear source label, such as `resource: iron-ore`,
  `offshore pump`, `boiler steam`, or `rocket launch: satellite`.
- Selection overlays should not offer recipe choices for a commodity whose only
  source is non-recipe.

Possible later UI:

- Add a source selector for commodities with multiple source kinds.
- Let users mark resource extraction, water, or steam as external inputs when
  they want to stop expansion.

## 9. Persistence

Avoid changing persisted plan schema until users can make explicit non-recipe
source choices. The first implementation can keep source selection implicit and
derived from the catalog.

If explicit source choices are later added, introduce a schema migration from
recipe-only choices to source choices:

- Keep `recipe_choices` for backward compatibility.
- Add `source_choices` keyed by commodity.
- Interpret old recipe choices as `ProductionSource::Recipe(recipe_id)`.

## 10. Note On Fuel Byproducts

Fuel byproducts are real non-recipe outputs. The base-game example is
`uranium-fuel-cell`, which has `burnt_result = depleted-uranium-fuel-cell`.

Do not include this in the first non-recipe source implementation unless power
or nuclear fuel accounting is being improved at the same time. Burnt results are
outputs of fuel consumption, not primary commodity production sources, so they
fit better as part of burner/reactor energy modeling and surplus accounting.

When implemented, expected behavior is:

- Fuel consumption records burnt-result surplus at the correct rate.
- Depleted uranium fuel cells can appear as a byproduct of nuclear fuel use.
- The planner should not choose fuel consumption as a normal way to produce a
  target commodity unless the user explicitly asks for byproduct accounting.
