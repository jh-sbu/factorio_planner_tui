use factorio_planner_tui::catalog::{
    Catalog, CatalogParts, Commodity, CommodityId, Finite, ItemId, Machine, MachineEnergySource,
    MachineId, Module, ModuleCategory, ModuleEffect, ModuleId, NonNegative, Positive, Product,
    Recipe, RecipeCategory, RecipeId,
};
use factorio_planner_tui::planner::{
    FactoryPlan, PlanEditError, PlannerError, ProductionStep, RateUnit, Target, calculate,
};
use proptest::prelude::*;

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
}

fn recipe_id(name: &str) -> RecipeId {
    RecipeId::new(name).expect("test recipe ID should be valid")
}

fn machine_id(name: &str) -> MachineId {
    MachineId::new(name).expect("test machine ID should be valid")
}

fn module_id(name: &str) -> ModuleId {
    ModuleId::new(name).expect("test module ID should be valid")
}

fn positive(value: f64) -> Positive {
    Positive::new(value).expect("test value should be positive")
}

fn finite(value: f64) -> Finite {
    Finite::new(value).expect("test value should be finite")
}

fn machine(
    name: &str,
    categories: impl IntoIterator<Item = RecipeCategory>,
    crafting_speed: f64,
) -> Machine {
    Machine::new(
        machine_id(name),
        categories,
        positive(crafting_speed),
        0,
        [],
        None,
        positive(90_000.0),
        MachineEnergySource::Electric {
            drain: NonNegative::new(0.0).unwrap(),
        },
    )
    .unwrap()
}

fn recipe(
    name: &str,
    category: &RecipeCategory,
    duration: f64,
    ingredients: Vec<(CommodityId, f64)>,
    product: CommodityId,
    product_amount: f64,
) -> Recipe {
    Recipe::new(
        recipe_id(name),
        category.clone(),
        positive(duration),
        ingredients
            .into_iter()
            .map(|(commodity, amount)| {
                factorio_planner_tui::catalog::Ingredient::new(commodity, positive(amount))
            })
            .collect(),
        vec![Product::new(product.clone(), positive(product_amount))],
        Some(product),
        true,
    )
    .unwrap()
}

fn recipe_with_products(
    name: &str,
    category: &RecipeCategory,
    duration: f64,
    ingredients: Vec<(CommodityId, f64)>,
    products: Vec<(CommodityId, f64)>,
    main_product: Option<CommodityId>,
) -> Recipe {
    Recipe::new(
        recipe_id(name),
        category.clone(),
        positive(duration),
        ingredients
            .into_iter()
            .map(|(commodity, amount)| {
                factorio_planner_tui::catalog::Ingredient::new(commodity, positive(amount))
            })
            .collect(),
        products
            .into_iter()
            .map(|(commodity, amount)| Product::new(commodity, positive(amount)))
            .collect(),
        main_product,
        true,
    )
    .unwrap()
}

fn hidden_recipe(
    name: &str,
    category: &RecipeCategory,
    ingredients: Vec<(CommodityId, f64)>,
    product: CommodityId,
) -> Recipe {
    Recipe::new(
        recipe_id(name),
        category.clone(),
        positive(1.0),
        ingredients
            .into_iter()
            .map(|(commodity, amount)| {
                factorio_planner_tui::catalog::Ingredient::new(commodity, positive(amount))
            })
            .collect(),
        vec![Product::new(product.clone(), positive(1.0))],
        Some(product),
        false,
    )
    .unwrap()
}

fn catalog(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn catalog_with_modules(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
    modules: Vec<Module>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        modules,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn target(commodity: CommodityId, rate_per_second: f64) -> Target {
    Target::new(commodity, rate_per_second).expect("test target should be valid")
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1.0e-10,
        "expected {expected}, got {actual}"
    );
}

fn shared_intermediate_catalog() -> (Catalog, CommodityId, CommodityId, CommodityId, CommodityId) {
    let ore = item("iron-ore");
    let plate = item("iron-plate");
    let gear = item("iron-gear-wheel");
    let pipe = item("pipe");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [ore.clone(), plate.clone(), gear.clone(), pipe.clone()],
        vec![
            recipe(
                "iron-plate",
                &crafting,
                1.0,
                vec![(ore.clone(), 1.0)],
                plate.clone(),
                1.0,
            ),
            recipe(
                "iron-gear-wheel",
                &crafting,
                0.5,
                vec![(plate.clone(), 2.0)],
                gear.clone(),
                1.0,
            ),
            recipe(
                "pipe",
                &crafting,
                0.5,
                vec![(plate.clone(), 1.0)],
                pipe.clone(),
                1.0,
            ),
        ],
        vec![machine("assembling-machine-1", [crafting], 1.0)],
    );
    (catalog, ore, plate, gear, pipe)
}

fn step_for<'a>(
    result: &'a factorio_planner_tui::planner::CalculationResult,
    commodity: &CommodityId,
) -> &'a ProductionStep {
    result
        .production_steps()
        .iter()
        .find(|step| step.planning_product() == commodity)
        .expect("expected production step")
}

fn rate_for(
    rates: &[factorio_planner_tui::planner::CommodityRate],
    commodity: &CommodityId,
) -> f64 {
    rates
        .iter()
        .find(|rate| rate.commodity() == commodity)
        .expect("expected commodity rate")
        .rate()
        .get()
}

fn configured_machine(
    name: &str,
    category: RecipeCategory,
    crafting_speed: f64,
    module_slots: u16,
    allowed_effects: impl IntoIterator<Item = ModuleEffect>,
    allowed_module_categories: Option<std::collections::BTreeSet<ModuleCategory>>,
) -> Machine {
    Machine::new(
        machine_id(name),
        [category],
        positive(crafting_speed),
        module_slots,
        allowed_effects,
        allowed_module_categories,
        positive(90_000.0),
        MachineEnergySource::Electric {
            drain: NonNegative::new(0.0).unwrap(),
        },
    )
    .unwrap()
}

fn test_module(
    name: &str,
    category: &str,
    speed: f64,
    productivity: f64,
    consumption: f64,
) -> Module {
    Module::new(
        module_id(name),
        ModuleCategory::new(category).unwrap(),
        finite(speed),
        finite(productivity),
        finite(consumption),
    )
}

#[test]
fn calculates_one_target_with_one_recipe_and_machine_without_mutating_inputs() {
    let gear = item("iron-gear-wheel");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [gear.clone()],
        vec![recipe(
            "iron-gear-wheel",
            &crafting,
            2.0,
            vec![],
            gear.clone(),
            2.0,
        )],
        vec![machine("assembling-machine-1", [crafting], 1.0)],
    );
    let plan = FactoryPlan::new(target(gear.clone(), 3.0));
    let original_catalog = catalog.clone();
    let original_plan = plan.clone();

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(catalog, original_catalog);
    assert_eq!(plan, original_plan);
    assert!(result.external_inputs().is_empty());
    assert_eq!(result.production_steps().len(), 1);
    let step = &result.production_steps()[0];
    assert_eq!(step.planning_product(), &gear);
    assert_eq!(step.recipe(), &recipe_id("iron-gear-wheel"));
    assert_eq!(step.machine(), &machine_id("assembling-machine-1"));
    assert_close(step.required_output_rate().get(), 3.0);
    assert_close(step.craft_rate().get(), 1.5);
    assert_close(step.fractional_machine_count().get(), 3.0);
    assert_eq!(step.installed_machine_count(), 3);
}

#[test]
fn recursively_expands_ingredients_and_accumulates_raw_inputs() {
    let ore = item("iron-ore");
    let plate = item("iron-plate");
    let gear = item("iron-gear-wheel");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [ore.clone(), plate.clone(), gear.clone()],
        vec![
            recipe(
                "iron-plate",
                &crafting,
                1.0,
                vec![(ore.clone(), 1.0)],
                plate.clone(),
                1.0,
            ),
            recipe(
                "iron-gear-wheel",
                &crafting,
                0.5,
                vec![(plate.clone(), 2.0)],
                gear.clone(),
                1.0,
            ),
        ],
        vec![machine("assembling-machine-1", [crafting], 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(gear.clone(), 3.0))).unwrap();

    assert_eq!(
        result
            .production_steps()
            .iter()
            .map(ProductionStep::planning_product)
            .collect::<Vec<_>>(),
        [&gear, &plate]
    );
    assert_close(
        result.production_steps()[0].required_output_rate().get(),
        3.0,
    );
    assert_close(
        result.production_steps()[1].required_output_rate().get(),
        6.0,
    );
    assert_eq!(result.external_inputs().len(), 1);
    assert_eq!(result.external_inputs()[0].commodity(), &ore);
    assert_close(result.external_inputs()[0].rate().get(), 6.0);
}

#[test]
fn reports_all_multi_product_outputs_and_secondary_surplus() {
    let plate = item("iron-plate");
    let slag = item("slag");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone(), slag.clone()],
        vec![recipe_with_products(
            "smelt-iron",
            &crafting,
            4.0,
            vec![],
            vec![(plate.clone(), 2.0), (slag.clone(), 3.0)],
            Some(plate.clone()),
        )],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 5.0))).unwrap();
    let step = step_for(&result, &plate);

    assert_close(step.craft_rate().get(), 2.5);
    assert_close(step.fractional_machine_count().get(), 10.0);
    assert_close(rate_for(step.products(), &plate), 5.0);
    assert_close(rate_for(step.products(), &slag), 7.5);
    assert_eq!(result.surplus().len(), 1);
    assert_close(rate_for(result.surplus(), &slag), 7.5);
}

#[test]
fn surplus_does_not_reduce_target_or_ingredient_demand() {
    let plate = item("iron-plate");
    let slag = item("slag");
    let brick = item("slag-brick");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone(), slag.clone(), brick.clone()],
        vec![
            recipe_with_products(
                "smelt-iron",
                &crafting,
                1.0,
                vec![],
                vec![(plate.clone(), 1.0), (slag.clone(), 2.0)],
                Some(plate.clone()),
            ),
            recipe(
                "slag-brick",
                &crafting,
                1.0,
                vec![(slag.clone(), 1.0)],
                brick.clone(),
                1.0,
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let mut plan = FactoryPlan::new(target(plate, 2.0)).with_external_inputs([slag.clone()]);
    plan.add_target(target(brick, 3.0));

    let result = calculate(&catalog, &plan).unwrap();

    assert_close(rate_for(result.surplus(), &slag), 4.0);
    assert_close(rate_for(result.external_inputs(), &slag), 3.0);
}

#[test]
fn choosing_another_planning_product_changes_multi_product_sizing() {
    let plate = item("iron-plate");
    let slag = item("slag");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let shared_recipe = recipe_with_products(
        "separate-ore",
        &crafting,
        1.0,
        vec![],
        vec![(plate.clone(), 2.0), (slag.clone(), 4.0)],
        None,
    );
    let catalog = catalog(
        [plate.clone(), slag.clone()],
        vec![shared_recipe],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let plate_result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 6.0))).unwrap();
    let slag_result = calculate(&catalog, &FactoryPlan::new(target(slag.clone(), 6.0))).unwrap();

    assert_close(step_for(&plate_result, &plate).craft_rate().get(), 3.0);
    assert_close(rate_for(plate_result.surplus(), &slag), 12.0);
    assert_close(step_for(&slag_result, &slag).craft_rate().get(), 1.5);
    assert_close(rate_for(slag_result.surplus(), &plate), 3.0);
}

#[test]
fn aggregates_secondary_surplus_deterministically() {
    let a = item("a");
    let b = item("b");
    let ash = item("ash");
    let slag = item("slag");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [a.clone(), b.clone(), ash.clone(), slag.clone()],
        vec![
            recipe_with_products(
                "make-a",
                &crafting,
                1.0,
                vec![],
                vec![(a.clone(), 1.0), (slag.clone(), 2.0), (ash.clone(), 1.0)],
                Some(a.clone()),
            ),
            recipe_with_products(
                "make-b",
                &crafting,
                1.0,
                vec![],
                vec![(b.clone(), 1.0), (slag.clone(), 3.0)],
                Some(b.clone()),
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let mut forward = FactoryPlan::new(target(a.clone(), 2.0));
    forward.add_target(target(b.clone(), 4.0));
    let mut reverse = FactoryPlan::new(target(b, 4.0));
    reverse.add_target(target(a, 2.0));

    let forward = calculate(&catalog, &forward).unwrap();
    let reverse = calculate(&catalog, &reverse).unwrap();

    assert_eq!(forward.surplus(), reverse.surplus());
    assert_eq!(
        forward
            .surplus()
            .iter()
            .map(factorio_planner_tui::planner::CommodityRate::commodity)
            .collect::<Vec<_>>(),
        [&ash, &slag]
    );
    assert_close(rate_for(forward.surplus(), &ash), 2.0);
    assert_close(rate_for(forward.surplus(), &slag), 16.0);
}

#[test]
fn combines_multiple_targets_and_shared_intermediates_once() {
    let (catalog, ore, plate, gear, pipe) = shared_intermediate_catalog();
    let mut plan = FactoryPlan::new(target(gear.clone(), 3.0));
    plan.add_target(target(pipe.clone(), 4.0));

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(plan.targets().len(), 2);
    assert_eq!(result.production_steps().len(), 3);
    assert_close(step_for(&result, &gear).required_output_rate().get(), 3.0);
    assert_close(step_for(&result, &pipe).required_output_rate().get(), 4.0);
    let plate_step = step_for(&result, &plate);
    assert_close(plate_step.required_output_rate().get(), 10.0);
    assert_close(plate_step.fractional_machine_count().get(), 10.0);
    assert_eq!(plate_step.installed_machine_count(), 10);
    assert_eq!(result.external_inputs().len(), 1);
    assert_eq!(result.external_inputs()[0].commodity(), &ore);
    assert_close(result.external_inputs()[0].rate().get(), 10.0);
}

#[test]
fn sums_duplicate_targets_before_rounding_machine_counts() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![recipe(
            "iron-plate",
            &crafting,
            1.0,
            vec![],
            plate.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 3.0)],
    );
    let mut duplicate_plan = FactoryPlan::new(target(plate.clone(), 1.0));
    duplicate_plan.add_target(target(plate.clone(), 1.0));
    let summed_plan = FactoryPlan::new(target(plate, 2.0));

    let duplicate_result = calculate(&catalog, &duplicate_plan).unwrap();
    let summed_result = calculate(&catalog, &summed_plan).unwrap();

    assert_eq!(duplicate_result, summed_result);
    assert_eq!(
        duplicate_result.production_steps()[0].installed_machine_count(),
        1
    );
}

#[test]
fn target_order_does_not_change_aggregate_results() {
    let (catalog, _, _, gear, pipe) = shared_intermediate_catalog();
    let mut forward = FactoryPlan::new(target(gear.clone(), 1.25));
    forward.add_target(target(pipe.clone(), 2.75));
    let mut reverse = FactoryPlan::new(target(pipe, 2.75));
    reverse.add_target(target(gear, 1.25));

    assert_eq!(
        calculate(&catalog, &forward).unwrap(),
        calculate(&catalog, &reverse).unwrap()
    );
}

#[test]
fn adds_replaces_and_removes_targets_without_invalid_partial_edits() {
    let gear = item("iron-gear-wheel");
    let pipe = item("pipe");
    let plate = item("iron-plate");
    let mut plan = FactoryPlan::new(target(gear.clone(), 1.0));

    plan.add_target(target(pipe.clone(), 2.0));
    assert_eq!(
        plan.targets()
            .iter()
            .map(Target::commodity)
            .collect::<Vec<_>>(),
        [&gear, &pipe]
    );

    let replaced = plan.replace_target(0, target(plate.clone(), 3.0)).unwrap();
    assert_eq!(replaced.commodity(), &gear);
    assert_eq!(plan.targets()[0].commodity(), &plate);

    let before_invalid_replace = plan.clone();
    assert_eq!(
        plan.replace_target(2, target(gear, 4.0)),
        Err(PlanEditError::TargetIndexOutOfBounds { index: 2, len: 2 })
    );
    assert_eq!(plan, before_invalid_replace);

    let removed = plan.remove_target(1).unwrap();
    assert_eq!(removed.commodity(), &pipe);
    let before_final_remove = plan.clone();
    assert_eq!(
        plan.remove_target(0),
        Err(PlanEditError::CannotRemoveLastTarget)
    );
    assert_eq!(plan, before_final_remove);
}

#[test]
fn explicit_external_boundaries_stop_recursive_expansion() {
    let ore = item("iron-ore");
    let plate = item("iron-plate");
    let gear = item("iron-gear-wheel");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [ore, plate.clone(), gear.clone()],
        vec![
            recipe(
                "iron-plate",
                &crafting,
                1.0,
                vec![(item("iron-ore"), 1.0)],
                plate.clone(),
                1.0,
            ),
            recipe(
                "iron-gear-wheel",
                &crafting,
                0.5,
                vec![(plate.clone(), 2.0)],
                gear.clone(),
                1.0,
            ),
        ],
        vec![machine("assembling-machine-1", [crafting], 1.0)],
    );
    let plan = FactoryPlan::new(target(gear, 3.0)).with_external_inputs([plate.clone()]);

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(result.production_steps().len(), 1);
    assert_eq!(result.external_inputs()[0].commodity(), &plate);
    assert_close(result.external_inputs()[0].rate().get(), 6.0);
}

#[test]
fn reports_fractional_and_rounded_machine_counts() {
    let plate = item("iron-plate");
    let smelting = RecipeCategory::new("smelting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![recipe(
            "iron-plate",
            &smelting,
            0.5,
            vec![],
            plate.clone(),
            1.0,
        )],
        vec![machine("stone-furnace", [smelting], 0.75)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate, 1.0))).unwrap();
    let step = &result.production_steps()[0];

    assert_close(step.fractional_machine_count().get(), 2.0 / 3.0);
    assert_eq!(step.installed_machine_count(), 1);
}

#[test]
fn converts_display_units_without_changing_base_rates() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![recipe(
            "iron-plate",
            &crafting,
            1.0,
            vec![],
            plate.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let plan = FactoryPlan::new(target(plate, 2.0)).with_display_rate_unit(RateUnit::Hour);

    let result = calculate(&catalog, &plan).unwrap();
    let base_rate = result.production_steps()[0].required_output_rate();

    assert_eq!(result.display_rate_unit(), RateUnit::Hour);
    assert_close(base_rate.get(), 2.0);
    assert_close(RateUnit::Second.convert_rate(base_rate), 2.0);
    assert_close(RateUnit::Minute.convert_rate(base_rate), 120.0);
    assert_close(RateUnit::Hour.convert_rate(base_rate), 7_200.0);
}

#[test]
fn rejects_unknown_targets() {
    let plate = item("iron-plate");
    let empty_catalog = catalog([], vec![], vec![]);
    assert_eq!(
        calculate(
            &empty_catalog,
            &FactoryPlan::new(target(plate.clone(), 1.0))
        ),
        Err(PlannerError::UnknownTarget {
            commodity: plate.clone()
        })
    );
}

#[test]
fn chooses_recipes_by_main_product_visibility_single_product_and_lexical_id() {
    let plate = item("iron-plate");
    let slag = item("slag");
    let stone = item("stone");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone(), slag.clone(), stone.clone()],
        vec![
            Recipe::new(
                recipe_id("a-visible-other-main"),
                crafting.clone(),
                positive(1.0),
                vec![],
                vec![
                    Product::new(plate.clone(), positive(1.0)),
                    Product::new(slag.clone(), positive(1.0)),
                ],
                Some(slag),
                true,
            )
            .unwrap(),
            Recipe::new(
                recipe_id("b-visible-single"),
                crafting.clone(),
                positive(1.0),
                vec![],
                vec![Product::new(plate.clone(), positive(1.0))],
                None,
                true,
            )
            .unwrap(),
            Recipe::new(
                recipe_id("c-visible-main"),
                crafting.clone(),
                positive(1.0),
                vec![],
                vec![
                    Product::new(plate.clone(), positive(1.0)),
                    Product::new(stone, positive(1.0)),
                ],
                Some(plate.clone()),
                true,
            )
            .unwrap(),
            hidden_recipe("hidden-main", &crafting, vec![], plate.clone()),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate, 1.0))).unwrap();

    assert_eq!(
        result.production_steps()[0].recipe(),
        &recipe_id("c-visible-main")
    );
}

#[test]
fn uses_lexical_recipe_id_for_equal_defaults_and_hidden_recipes_as_fallback() {
    let visible_product = item("visible-product");
    let hidden_product = item("hidden-product");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [visible_product.clone(), hidden_product.clone()],
        vec![
            recipe(
                "z-visible",
                &crafting,
                1.0,
                vec![],
                visible_product.clone(),
                1.0,
            ),
            recipe(
                "a-visible",
                &crafting,
                1.0,
                vec![],
                visible_product.clone(),
                1.0,
            ),
            hidden_recipe("z-hidden", &crafting, vec![], hidden_product.clone()),
            hidden_recipe("a-hidden", &crafting, vec![], hidden_product.clone()),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let visible = calculate(&catalog, &FactoryPlan::new(target(visible_product, 1.0))).unwrap();
    let hidden = calculate(&catalog, &FactoryPlan::new(target(hidden_product, 1.0))).unwrap();

    assert_eq!(
        visible.production_steps()[0].recipe(),
        &recipe_id("a-visible")
    );
    assert_eq!(
        hidden.production_steps()[0].recipe(),
        &recipe_id("a-hidden")
    );
}

#[test]
fn explicit_recipe_choice_overrides_the_default_and_can_be_cleared() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![
            recipe("a-iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0),
            hidden_recipe("b-iron-plate", &crafting, vec![], plate.clone()),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));
    assert_eq!(
        plan.set_recipe_choice(plate.clone(), recipe_id("b-iron-plate")),
        None
    );
    assert_eq!(plan.recipe_choice(&plate), Some(&recipe_id("b-iron-plate")));
    let overridden = calculate(&catalog, &plan).unwrap();
    assert_eq!(
        overridden.production_steps()[0].recipe(),
        &recipe_id("b-iron-plate")
    );

    assert_eq!(
        plan.clear_recipe_choice(&plate),
        Some(recipe_id("b-iron-plate"))
    );
    assert_eq!(plan.recipe_choice(&plate), None);
    let defaulted = calculate(&catalog, &plan).unwrap();
    assert_eq!(
        defaulted.production_steps()[0].recipe(),
        &recipe_id("a-iron-plate")
    );
}

#[test]
fn rejects_missing_unsupported_and_wrong_product_recipe_choices() {
    let plate = item("iron-plate");
    let gear = item("iron-gear-wheel");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone(), gear.clone()],
        vec![
            recipe("iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0),
            recipe(
                "unsupported-plate",
                &crafting,
                1.0,
                vec![],
                plate.clone(),
                1.0,
            )
            .with_supported(false),
            recipe("iron-gear-wheel", &crafting, 1.0, vec![], gear, 1.0),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let mut missing = FactoryPlan::new(target(plate.clone(), 1.0));
    missing.set_recipe_choice(plate.clone(), recipe_id("missing"));
    assert_eq!(
        calculate(&catalog, &missing),
        Err(PlannerError::MissingRecipeChoice {
            commodity: plate.clone(),
            recipe: recipe_id("missing"),
        })
    );

    let mut unsupported = FactoryPlan::new(target(plate.clone(), 1.0));
    unsupported.set_recipe_choice(plate.clone(), recipe_id("unsupported-plate"));
    assert_eq!(
        calculate(&catalog, &unsupported),
        Err(PlannerError::UnsupportedRecipeChoice {
            commodity: plate.clone(),
            recipe: recipe_id("unsupported-plate"),
        })
    );

    let mut wrong_product = FactoryPlan::new(target(plate.clone(), 1.0));
    wrong_product.set_recipe_choice(plate.clone(), recipe_id("iron-gear-wheel"));
    assert_eq!(
        calculate(&catalog, &wrong_product),
        Err(PlannerError::RecipeDoesNotProduceCommodity {
            commodity: plate,
            recipe: recipe_id("iron-gear-wheel"),
        })
    );
}

#[test]
fn treats_a_commodity_with_only_unsupported_recipes_as_external() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![
            recipe(
                "unsupported-plate",
                &crafting,
                1.0,
                vec![],
                plate.clone(),
                1.0,
            )
            .with_supported(false),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 2.0))).unwrap();

    assert!(result.production_steps().is_empty());
    assert_eq!(result.external_inputs()[0].commodity(), &plate);
    assert_close(result.external_inputs()[0].rate().get(), 2.0);
}

#[test]
fn rejects_recipes_without_compatible_machines() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let plate_recipe = recipe("iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0);
    let without_machine = catalog([plate.clone()], vec![plate_recipe.clone()], vec![]);
    assert_eq!(
        calculate(
            &without_machine,
            &FactoryPlan::new(target(plate.clone(), 1.0))
        ),
        Err(PlannerError::NoCompatibleMachine {
            recipe: recipe_id("iron-plate"),
            category: crafting.clone(),
        })
    );
}

#[test]
fn chooses_fastest_machine_with_lexical_tie_breaking_and_allows_overrides() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let recipe = recipe("iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0);
    let catalog = catalog(
        [plate.clone()],
        vec![recipe],
        vec![
            machine("slow", [crafting.clone()], 1.0),
            machine("z-fast", [crafting.clone()], 2.0),
            machine("a-fast", [crafting], 2.0),
        ],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 4.0));

    let defaulted = calculate(&catalog, &plan).unwrap();
    assert_eq!(
        step_for(&defaulted, &plate).machine(),
        &machine_id("a-fast")
    );
    assert_close(
        step_for(&defaulted, &plate)
            .fractional_machine_count()
            .get(),
        2.0,
    );

    assert_eq!(
        plan.set_machine_choice(recipe_id("iron-plate"), machine_id("slow")),
        None
    );
    let overridden = calculate(&catalog, &plan).unwrap();
    assert_eq!(step_for(&overridden, &plate).machine(), &machine_id("slow"));
    assert_close(
        step_for(&overridden, &plate)
            .fractional_machine_count()
            .get(),
        4.0,
    );
    assert_eq!(
        plan.clear_machine_choice(&recipe_id("iron-plate")),
        Some(machine_id("slow"))
    );
}

#[test]
fn rejects_stale_and_incompatible_machine_choices() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let smelting = RecipeCategory::new("smelting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![recipe(
            "iron-plate",
            &crafting,
            1.0,
            vec![],
            plate.clone(),
            1.0,
        )],
        vec![
            machine("assembler", [crafting.clone()], 1.0),
            machine("furnace", [smelting], 1.0),
        ],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    plan.set_machine_choice(recipe_id("iron-plate"), machine_id("missing"));
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MissingMachineChoice {
            recipe: recipe_id("iron-plate"),
            machine: machine_id("missing"),
        })
    );

    plan.set_machine_choice(recipe_id("iron-plate"), machine_id("furnace"));
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::IncompatibleMachineChoice {
            recipe: recipe_id("iron-plate"),
            machine: machine_id("furnace"),
            category: crafting,
        })
    );
}

#[test]
fn applies_machine_module_speed_productivity_and_consumption_effects() {
    let ore = item("ore");
    let plate = item("plate");
    let slag = item("slag");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let productivity_category = ModuleCategory::new("productivity").unwrap();
    let recipe = Recipe::new(
        recipe_id("plate"),
        crafting.clone(),
        positive(1.0),
        vec![factorio_planner_tui::catalog::Ingredient::new(
            ore.clone(),
            positive(2.0),
        )],
        vec![
            Product::new(plate.clone(), positive(1.0)),
            Product::new(slag.clone(), positive(2.0))
                .with_productivity_amount(NonNegative::new(0.0).unwrap())
                .unwrap(),
        ],
        Some(plate.clone()),
        true,
    )
    .unwrap()
    .with_module_policy(
        [
            ModuleEffect::Speed,
            ModuleEffect::Productivity,
            ModuleEffect::Consumption,
        ],
        Some([productivity_category.clone()].into_iter().collect()),
        NonNegative::new(0.1).unwrap(),
    );
    let catalog = catalog_with_modules(
        [ore.clone(), plate.clone(), slag.clone()],
        vec![recipe],
        vec![configured_machine(
            "assembler",
            crafting,
            1.0,
            2,
            [
                ModuleEffect::Speed,
                ModuleEffect::Productivity,
                ModuleEffect::Consumption,
            ],
            Some([productivity_category].into_iter().collect()),
        )],
        vec![test_module("combined", "productivity", 0.5, 0.2, -0.9)],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 11.0));
    plan.set_modules(plate.clone(), [module_id("combined")]);

    let result = calculate(&catalog, &plan).unwrap();
    let step = step_for(&result, &plate);

    assert_eq!(step.modules(), &[module_id("combined")]);
    assert_close(step.speed_multiplier().get(), 1.5);
    assert_close(step.productivity_effect().get(), 0.1);
    assert_close(step.consumption_multiplier().get(), 0.2);
    assert_close(step.craft_rate().get(), 10.0);
    assert_close(step.fractional_machine_count().get(), 10.0 / 1.5);
    assert_close(rate_for(step.ingredients(), &ore), 20.0);
    assert_close(rate_for(step.products(), &plate), 11.0);
    assert_close(rate_for(step.products(), &slag), 20.0);
    assert_close(rate_for(result.surplus(), &slag), 20.0);
}

#[test]
fn rejects_invalid_module_configurations() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let speed_category = ModuleCategory::new("speed").unwrap();
    let productivity_category = ModuleCategory::new("productivity").unwrap();
    let recipe = recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0).with_module_policy(
        [ModuleEffect::Speed],
        Some([speed_category.clone()].into_iter().collect()),
        NonNegative::new(3.0).unwrap(),
    );
    let catalog = catalog_with_modules(
        [plate.clone()],
        vec![recipe],
        vec![configured_machine(
            "assembler",
            crafting,
            1.0,
            1,
            [ModuleEffect::Speed],
            Some([speed_category].into_iter().collect()),
        )],
        vec![
            test_module("speed", "speed", 0.2, 0.0, 0.0),
            test_module("productivity", "productivity", 0.0, 0.1, 0.0),
        ],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    plan.set_modules(plate.clone(), [module_id("speed"), module_id("speed")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::TooManyModules {
            commodity: plate.clone(),
            machine: machine_id("assembler"),
            selected: 2,
            slots: 1,
        })
    );

    plan.set_modules(plate.clone(), [module_id("missing")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MissingModuleChoice {
            commodity: plate.clone(),
            module: module_id("missing"),
        })
    );

    plan.set_modules(plate.clone(), [module_id("productivity")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MachineDisallowsModuleCategory {
            commodity: plate,
            machine: machine_id("assembler"),
            module: module_id("productivity"),
            category: productivity_category,
        })
    );
}

#[test]
fn validates_machine_and_recipe_module_effect_restrictions() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let speed_category = ModuleCategory::new("speed").unwrap();
    let speed_module = test_module("speed", "speed", 0.2, 0.0, 0.0);

    let machine_restricted = catalog_with_modules(
        [plate.clone()],
        vec![
            recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0).with_module_policy(
                [ModuleEffect::Speed],
                None,
                NonNegative::new(3.0).unwrap(),
            ),
        ],
        vec![configured_machine(
            "assembler",
            crafting.clone(),
            1.0,
            1,
            [],
            Some([speed_category.clone()].into_iter().collect()),
        )],
        vec![speed_module.clone()],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));
    plan.set_modules(plate.clone(), [module_id("speed")]);
    assert_eq!(
        calculate(&machine_restricted, &plan),
        Err(PlannerError::MachineDisallowsModuleEffect {
            commodity: plate.clone(),
            machine: machine_id("assembler"),
            module: module_id("speed"),
            effect: ModuleEffect::Speed,
        })
    );

    let recipe_restricted = catalog_with_modules(
        [plate.clone()],
        vec![
            recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0).with_module_policy(
                [],
                None,
                NonNegative::new(3.0).unwrap(),
            ),
        ],
        vec![configured_machine(
            "assembler",
            crafting,
            1.0,
            1,
            [ModuleEffect::Speed],
            Some([speed_category].into_iter().collect()),
        )],
        vec![speed_module],
    );
    assert_eq!(
        calculate(&recipe_restricted, &plan),
        Err(PlannerError::RecipeDisallowsModuleEffect {
            commodity: plate,
            recipe: recipe_id("plate"),
            module: module_id("speed"),
            effect: ModuleEffect::Speed,
        })
    );
}

#[test]
fn rejects_recipe_category_and_unsupported_module_choices() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let speed_category = ModuleCategory::new("speed").unwrap();
    let productivity_category = ModuleCategory::new("productivity").unwrap();
    let recipe = recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0).with_module_policy(
        [ModuleEffect::Speed],
        Some([productivity_category.clone()].into_iter().collect()),
        NonNegative::new(3.0).unwrap(),
    );
    let catalog = catalog_with_modules(
        [plate.clone()],
        vec![recipe],
        vec![configured_machine(
            "assembler",
            crafting,
            1.0,
            1,
            [ModuleEffect::Speed],
            None,
        )],
        vec![
            test_module("speed", "speed", 0.2, 0.0, 0.0),
            test_module("future", "productivity", 0.2, 0.0, 0.0)
                .with_unsupported_effects(["future-effect".into()]),
        ],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    plan.set_modules(plate.clone(), [module_id("speed")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::RecipeDisallowsModuleCategory {
            commodity: plate.clone(),
            recipe: recipe_id("plate"),
            module: module_id("speed"),
            category: speed_category,
        })
    );

    plan.set_modules(plate.clone(), [module_id("future")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::UnsupportedModuleChoice {
            commodity: plate,
            module: module_id("future"),
        })
    );
}

#[test]
fn module_loadouts_are_canonicalized_without_losing_duplicate_slots() {
    let plate = item("plate");
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    assert_eq!(
        plan.set_modules(
            plate.clone(),
            [
                module_id("z-speed"),
                module_id("a-speed"),
                module_id("z-speed")
            ],
        ),
        None
    );
    assert_eq!(
        plan.modules_for(&plate),
        &[
            module_id("a-speed"),
            module_id("z-speed"),
            module_id("z-speed")
        ]
    );
    assert_eq!(
        plan.clear_modules(&plate),
        Some(vec![
            module_id("a-speed"),
            module_id("z-speed"),
            module_id("z-speed")
        ])
    );
    assert!(plan.modules_for(&plate).is_empty());
}

#[test]
fn rejects_dependency_cycles_with_the_complete_path() {
    let a = item("a");
    let b = item("b");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [a.clone(), b.clone()],
        vec![
            recipe(
                "make-a",
                &crafting,
                1.0,
                vec![(b.clone(), 1.0)],
                a.clone(),
                1.0,
            ),
            recipe(
                "make-b",
                &crafting,
                1.0,
                vec![(a.clone(), 1.0)],
                b.clone(),
                1.0,
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );

    assert_eq!(
        calculate(&catalog, &FactoryPlan::new(target(a.clone(), 1.0))),
        Err(PlannerError::Cycle {
            path: vec![a.clone(), b, a]
        })
    );
}

#[test]
fn rejects_direct_dependency_cycles_with_the_complete_path() {
    let a = item("a");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [a.clone()],
        vec![recipe(
            "make-a",
            &crafting,
            1.0,
            vec![(a.clone(), 1.0)],
            a.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 1.0)],
    );

    assert_eq!(
        calculate(&catalog, &FactoryPlan::new(target(a.clone(), 1.0))),
        Err(PlannerError::Cycle {
            path: vec![a.clone(), a]
        })
    );
}

#[test]
fn resolves_a_cycle_with_an_alternate_recipe_choice() {
    let a = item("a");
    let b = item("b");
    let ore = item("ore");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [a.clone(), b.clone(), ore.clone()],
        vec![
            recipe(
                "a-cyclic",
                &crafting,
                1.0,
                vec![(b.clone(), 1.0)],
                a.clone(),
                1.0,
            ),
            recipe(
                "z-safe",
                &crafting,
                1.0,
                vec![(ore.clone(), 1.0)],
                a.clone(),
                1.0,
            ),
            recipe("make-b", &crafting, 1.0, vec![(a.clone(), 1.0)], b, 1.0),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let mut plan = FactoryPlan::new(target(a.clone(), 1.0));

    assert!(matches!(
        calculate(&catalog, &plan),
        Err(PlannerError::Cycle { .. })
    ));

    plan.set_recipe_choice(a.clone(), recipe_id("z-safe"));
    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(step_for(&result, &a).recipe(), &recipe_id("z-safe"));
    assert_eq!(result.external_inputs()[0].commodity(), &ore);
}

#[test]
fn resolves_a_cycle_with_an_external_boundary_even_when_a_recipe_is_selected() {
    let a = item("a");
    let b = item("b");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [a.clone(), b.clone()],
        vec![
            recipe(
                "make-a",
                &crafting,
                1.0,
                vec![(b.clone(), 1.0)],
                a.clone(),
                1.0,
            ),
            recipe(
                "make-b",
                &crafting,
                1.0,
                vec![(a.clone(), 1.0)],
                b.clone(),
                1.0,
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let mut plan = FactoryPlan::new(target(a.clone(), 1.0)).with_external_inputs([b.clone()]);
    plan.set_recipe_choice(b.clone(), recipe_id("make-b"));

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(result.production_steps().len(), 1);
    assert_eq!(result.external_inputs()[0].commodity(), &b);
}

proptest! {
    #[test]
    fn calculation_scales_linearly(
        gear_rate in 0.01_f64..1_000.0,
        pipe_rate in 0.01_f64..1_000.0,
        scale in 0.01_f64..100.0,
    ) {
        let (catalog, _, _, gear, pipe) = shared_intermediate_catalog();
        let mut base_plan = FactoryPlan::new(target(gear.clone(), gear_rate));
        base_plan.add_target(target(pipe.clone(), pipe_rate));
        let mut scaled_plan = FactoryPlan::new(target(gear, gear_rate * scale));
        scaled_plan.add_target(target(pipe, pipe_rate * scale));

        let base = calculate(&catalog, &base_plan).unwrap();
        let scaled = calculate(&catalog, &scaled_plan).unwrap();

        for (base_step, scaled_step) in base
            .production_steps()
            .iter()
            .zip(scaled.production_steps())
        {
            prop_assert_eq!(
                base_step.planning_product(),
                scaled_step.planning_product()
            );
            prop_assert!(
                (scaled_step.required_output_rate().get()
                    - base_step.required_output_rate().get() * scale)
                    .abs()
                    < 1.0e-8
            );
            prop_assert!(
                (scaled_step.craft_rate().get() - base_step.craft_rate().get() * scale).abs()
                    < 1.0e-8
            );
            prop_assert!(
                (scaled_step.fractional_machine_count().get()
                    - base_step.fractional_machine_count().get() * scale)
                    .abs()
                    < 1.0e-8
            );
        }
        for (base_input, scaled_input) in
            base.external_inputs().iter().zip(scaled.external_inputs())
        {
            prop_assert_eq!(base_input.commodity(), scaled_input.commodity());
            prop_assert!(
                (scaled_input.rate().get() - base_input.rate().get() * scale).abs() < 1.0e-8
            );
        }
    }

    #[test]
    fn calculated_rates_are_finite_and_positive(
        gear_rate in 0.01_f64..1_000_000.0,
        pipe_rate in 0.01_f64..1_000_000.0,
    ) {
        let (catalog, _, _, gear, pipe) = shared_intermediate_catalog();
        let mut plan = FactoryPlan::new(target(gear, gear_rate));
        plan.add_target(target(pipe, pipe_rate));

        let result = calculate(&catalog, &plan).unwrap();

        for step in result.production_steps() {
            prop_assert!(step.required_output_rate().get().is_finite());
            prop_assert!(step.required_output_rate().get() > 0.0);
            prop_assert!(step.craft_rate().get().is_finite());
            prop_assert!(step.craft_rate().get() > 0.0);
            prop_assert!(step.fractional_machine_count().get().is_finite());
            prop_assert!(step.fractional_machine_count().get() > 0.0);
            prop_assert!(step.installed_machine_count() > 0);
            for ingredient in step.ingredients() {
                prop_assert!(ingredient.rate().get().is_finite());
                prop_assert!(ingredient.rate().get() > 0.0);
            }
        }
        for input in result.external_inputs() {
            prop_assert!(input.rate().get().is_finite());
            prop_assert!(input.rate().get() > 0.0);
        }
    }
}
