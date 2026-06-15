use factorio_planner_tui::catalog::{
    Catalog, CatalogParts, Commodity, CommodityId, ItemId, Machine, MachineEnergySource, MachineId,
    NonNegative, Positive, Product, Recipe, RecipeCategory, RecipeId,
};
use factorio_planner_tui::planner::{
    FactoryPlan, PlannerError, ProductionStep, RateUnit, Target, calculate,
};

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
}

fn recipe_id(name: &str) -> RecipeId {
    RecipeId::new(name).expect("test recipe ID should be valid")
}

fn machine_id(name: &str) -> MachineId {
    MachineId::new(name).expect("test machine ID should be valid")
}

fn positive(value: f64) -> Positive {
    Positive::new(value).expect("test value should be positive")
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

fn target(commodity: CommodityId, rate_per_second: f64) -> Target {
    Target::new(commodity, rate_per_second).expect("test target should be valid")
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1.0e-10,
        "expected {expected}, got {actual}"
    );
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
fn rejects_unknown_targets_and_ambiguous_recipes() {
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
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

    let ambiguous = catalog(
        [plate.clone()],
        vec![
            recipe("a-iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0),
            recipe("b-iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0),
        ],
        vec![machine("assembler", [crafting], 1.0)],
    );
    assert_eq!(
        calculate(&ambiguous, &FactoryPlan::new(target(plate.clone(), 1.0))),
        Err(PlannerError::AmbiguousRecipes {
            commodity: plate,
            recipes: vec![recipe_id("a-iron-plate"), recipe_id("b-iron-plate")],
        })
    );
}

#[test]
fn rejects_missing_and_ambiguous_compatible_machines() {
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

    let ambiguous = catalog(
        [plate.clone()],
        vec![plate_recipe],
        vec![
            machine("a-assembler", [crafting.clone()], 1.0),
            machine("b-assembler", [crafting], 2.0),
        ],
    );
    assert_eq!(
        calculate(&ambiguous, &FactoryPlan::new(target(plate, 1.0))),
        Err(PlannerError::AmbiguousMachines {
            recipe: recipe_id("iron-plate"),
            machines: vec![machine_id("a-assembler"), machine_id("b-assembler")],
        })
    );
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
