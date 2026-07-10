use factorio_planner_tui::catalog::{
    Belt, BeltId, Catalog, CatalogParts, Commodity, CommodityId, Finite, FluidId, FluidSource,
    FluidSourceId, FluidSourceKind, Fuel, FuelCategory, FuelId, Ingredient, ItemId, Machine,
    MachineEnergySource, MachineId, MiningMachine, MiningMachineId, Module, ModuleCategory,
    ModuleEffect, ModuleId, NonNegative, Positive, Product, ProductionSource, Recipe,
    RecipeCategory, RecipeId, ResourceCategory, ResourceSource, ResourceSourceId,
    RocketLaunchSource, RocketLaunchSourceId, UnsupportedEnergySource,
};
use factorio_planner_tui::planner::{
    DependencyNodeKind, FactoryPlan, PlanEditError, PlannerError, ProductionStep, RateUnit,
    StepEnergy, Target, calculate,
};
use proptest::prelude::*;

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
}

fn fluid(name: &str) -> CommodityId {
    CommodityId::Fluid(FluidId::new(name).expect("test fluid ID should be valid"))
}

fn item_id(name: &str) -> ItemId {
    ItemId::new(name).expect("test item ID should be valid")
}

fn recipe_id(name: &str) -> RecipeId {
    RecipeId::new(name).expect("test recipe ID should be valid")
}

fn machine_id(name: &str) -> MachineId {
    MachineId::new(name).expect("test machine ID should be valid")
}

fn mining_machine_id(name: &str) -> MiningMachineId {
    MiningMachineId::new(name).expect("test mining machine ID should be valid")
}

fn module_id(name: &str) -> ModuleId {
    ModuleId::new(name).expect("test module ID should be valid")
}

fn fuel_id(name: &str) -> FuelId {
    FuelId::new(name).expect("test fuel ID should be valid")
}

fn belt_id(name: &str) -> BeltId {
    BeltId::new(name).expect("test belt ID should be valid")
}

fn resource_id(name: &str) -> ResourceSourceId {
    ResourceSourceId::new(name).expect("test resource source ID should be valid")
}

fn fluid_source_id(name: &str) -> FluidSourceId {
    FluidSourceId::new(name).expect("test fluid source ID should be valid")
}

fn rocket_launch_id(name: &str) -> RocketLaunchSourceId {
    RocketLaunchSourceId::new(name).expect("test rocket launch source ID should be valid")
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

fn catalog_with_energy(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
    modules: Vec<Module>,
    fuels: Vec<Fuel>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        modules,
        fuels,
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

fn test_fuel(name: &str, category: &str, value: f64, burnt_result: Option<&str>) -> Fuel {
    Fuel::new(
        fuel_id(name),
        item_id(name),
        FuelCategory::new(category).unwrap(),
        positive(value),
        burnt_result.map(item_id),
    )
}

fn belt(name: &str, throughput: f64) -> Belt {
    Belt::new(belt_id(name), positive(throughput))
}

fn catalog_with_belts(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
    belts: Vec<Belt>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        belts,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn catalog_with_resources(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
    resource_sources: Vec<ResourceSource>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        resource_sources,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn catalog_with_resources_and_miners(
    commodities: impl IntoIterator<Item = CommodityId>,
    resource_sources: Vec<ResourceSource>,
    mining_machines: Vec<MiningMachine>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources,
        mining_machines,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn catalog_with_fluid_sources(
    commodities: impl IntoIterator<Item = CommodityId>,
    recipes: Vec<Recipe>,
    machines: Vec<Machine>,
    fuels: Vec<Fuel>,
    fluid_sources: Vec<FluidSource>,
) -> Catalog {
    Catalog::try_from_parts(CatalogParts {
        commodities: commodities
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        recipes,
        machines,
        fuels,
        fluid_sources,
        ..CatalogParts::default()
    })
    .unwrap()
}

fn offshore_pump_source(name: &str, product: CommodityId, rate: f64) -> FluidSource {
    FluidSource::new(
        fluid_source_id(name),
        FluidSourceKind::OffshorePump,
        vec![Product::new(product, positive(rate))],
        vec![],
        None,
        None,
    )
    .unwrap()
}

fn burner_boiler_source(
    name: &str,
    input: CommodityId,
    output: CommodityId,
    rate: f64,
) -> FluidSource {
    FluidSource::new(
        fluid_source_id(name),
        FluidSourceKind::BoilerSteam,
        vec![Product::new(output, positive(rate))],
        vec![Ingredient::new(input, positive(rate))],
        Some(MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(1.0),
        }),
        Some(positive(1_800_000.0)),
    )
    .unwrap()
}

fn resource_source(
    name: &str,
    product: CommodityId,
    amount: f64,
    required_fluid: Option<(CommodityId, f64)>,
) -> ResourceSource {
    ResourceSource::new(
        resource_id(name),
        ResourceCategory::new("basic-solid").unwrap(),
        false,
        positive(1.0),
        vec![Product::new(product, positive(amount))],
        required_fluid.map(|(commodity, amount)| {
            factorio_planner_tui::catalog::Ingredient::new(commodity, positive(amount))
        }),
    )
    .unwrap()
}

fn resource_source_with_category(
    name: &str,
    category: &ResourceCategory,
    product: CommodityId,
    amount: f64,
    mining_time: f64,
) -> ResourceSource {
    ResourceSource::new(
        resource_id(name),
        category.clone(),
        false,
        positive(mining_time),
        vec![Product::new(product, positive(amount))],
        None,
    )
    .unwrap()
}

fn mining_machine(name: &str, category: &ResourceCategory, mining_speed: f64) -> MiningMachine {
    MiningMachine::new(
        mining_machine_id(name),
        [category.clone()],
        positive(mining_speed),
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

fn configured_mining_machine(
    name: &str,
    category: &ResourceCategory,
    mining_speed: f64,
    energy_usage: f64,
    energy_source: MachineEnergySource,
) -> MiningMachine {
    MiningMachine::new(
        mining_machine_id(name),
        [category.clone()],
        positive(mining_speed),
        0,
        [],
        None,
        positive(energy_usage),
        energy_source,
    )
    .unwrap()
}

fn modular_mining_machine(
    name: &str,
    category: &ResourceCategory,
    mining_speed: f64,
    module_slots: u16,
    allowed_effects: impl IntoIterator<Item = ModuleEffect>,
    allowed_module_categories: Option<std::collections::BTreeSet<ModuleCategory>>,
    energy_usage: f64,
    energy_source: MachineEnergySource,
) -> MiningMachine {
    MiningMachine::new(
        mining_machine_id(name),
        [category.clone()],
        positive(mining_speed),
        module_slots,
        allowed_effects,
        allowed_module_categories,
        positive(energy_usage),
        energy_source,
    )
    .unwrap()
}

fn rocket_launch_source(
    name: &str,
    launched_item: &str,
    product: CommodityId,
    product_amount: f64,
    rocket_recipe: &str,
    rocket_parts_required: f64,
) -> RocketLaunchSource {
    RocketLaunchSource::new(
        rocket_launch_id(name),
        item_id(launched_item),
        vec![Product::new(product, positive(product_amount))],
        recipe_id(rocket_recipe),
        positive(rocket_parts_required),
    )
    .unwrap()
}

fn belt_equivalent_for<'a>(
    equivalents: &'a [factorio_planner_tui::planner::BeltEquivalent],
    commodity: &CommodityId,
) -> &'a factorio_planner_tui::planner::BeltEquivalent {
    equivalents
        .iter()
        .find(|equivalent| equivalent.commodity() == commodity)
        .expect("expected belt equivalent")
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
fn extracts_resource_targets_without_unexplained_external_inputs() {
    let ore = item("iron-ore");
    let catalog = catalog_with_resources(
        [ore.clone()],
        vec![],
        vec![],
        vec![resource_source("iron-ore", ore.clone(), 2.0, None)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore.clone(), 6.0))).unwrap();

    assert!(result.production_steps().is_empty());
    assert!(result.external_inputs().is_empty());
    assert_eq!(result.extraction_steps().len(), 1);
    let step = &result.extraction_steps()[0];
    assert_eq!(step.planning_product(), &ore);
    assert_eq!(
        step.source(),
        &factorio_planner_tui::catalog::ProductionSource::Resource(resource_id("iron-ore"))
    );
    assert_close(step.required_output_rate().get(), 6.0);
    assert_close(step.extraction_rate().get(), 3.0);
    assert_close(rate_for(step.products(), &ore), 6.0);

    let tree = &result.dependency_trees()[0];
    assert_eq!(tree.kind(), DependencyNodeKind::Production);
    assert!(tree.recipe().is_none());
    assert!(tree.machine().is_none());
    assert!(tree.fractional_machine_count().is_none());
}

#[test]
fn resource_extraction_reports_default_miner_counts() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            2.0,
        )],
        vec![mining_machine("electric-mining-drill", &basic_solid, 0.5)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore.clone(), 9.0))).unwrap();

    let step = &result.extraction_steps()[0];
    assert_eq!(
        step.mining_machine(),
        Some(&mining_machine_id("electric-mining-drill"))
    );
    assert_close(step.extraction_rate().get(), 9.0);
    assert_close(step.fractional_machine_count().unwrap().get(), 36.0);
    assert_eq!(step.installed_machine_count(), Some(36));
}

#[test]
fn resource_extraction_uses_fastest_default_miner_with_lexical_tie_break() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        vec![
            mining_machine("a-slower-miner", &basic_solid, 0.25),
            mining_machine("z-fast-miner", &basic_solid, 1.0),
            mining_machine("a-fast-miner", &basic_solid, 1.0),
        ],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore, 2.0))).unwrap();
    let step = &result.extraction_steps()[0];

    assert_eq!(
        step.mining_machine(),
        Some(&mining_machine_id("a-fast-miner"))
    );
    assert_close(step.fractional_machine_count().unwrap().get(), 2.0);
    assert_eq!(step.installed_machine_count(), Some(2));
}

#[test]
fn explicit_miner_selection_changes_resource_machine_count() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        vec![
            mining_machine("fast-miner", &basic_solid, 1.0),
            mining_machine("slow-miner", &basic_solid, 0.25),
        ],
    );
    let mut plan = FactoryPlan::new(target(ore.clone(), 2.0));
    plan.set_miner_choice(ore, mining_machine_id("slow-miner"));

    let result = calculate(&catalog, &plan).unwrap();
    let step = &result.extraction_steps()[0];

    assert_eq!(
        step.mining_machine(),
        Some(&mining_machine_id("slow-miner"))
    );
    assert_close(step.fractional_machine_count().unwrap().get(), 8.0);
    assert_eq!(step.installed_machine_count(), Some(8));
}

#[test]
fn missing_explicit_miner_selection_fails() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        vec![mining_machine("electric-mining-drill", &basic_solid, 0.5)],
    );
    let mut plan = FactoryPlan::new(target(ore.clone(), 2.0));
    plan.set_miner_choice(ore.clone(), mining_machine_id("missing-miner"));

    assert_eq!(
        calculate(&catalog, &plan).unwrap_err(),
        PlannerError::MissingMinerChoice {
            commodity: ore,
            miner: mining_machine_id("missing-miner"),
        }
    );
}

#[test]
fn incompatible_explicit_miner_selection_fails() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let hard_ore = ResourceCategory::new("hard-ore").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        vec![mining_machine("hard-ore-miner", &hard_ore, 0.5)],
    );
    let mut plan = FactoryPlan::new(target(ore.clone(), 2.0));
    plan.set_miner_choice(ore.clone(), mining_machine_id("hard-ore-miner"));

    assert_eq!(
        calculate(&catalog, &plan).unwrap_err(),
        PlannerError::IncompatibleMinerChoice {
            commodity: ore,
            miner: mining_machine_id("hard-ore-miner"),
            category: basic_solid,
        }
    );
}

#[test]
fn pumpjack_style_resource_extraction_uses_mining_machine_counts() {
    let crude_oil = fluid("crude-oil");
    let oil = ResourceCategory::new("basic-fluid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [crude_oil.clone()],
        vec![resource_source_with_category(
            "crude-oil",
            &oil,
            crude_oil.clone(),
            10.0,
            1.0,
        )],
        vec![mining_machine("pumpjack", &oil, 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(crude_oil, 60.0))).unwrap();
    let step = &result.extraction_steps()[0];

    assert_eq!(step.mining_machine(), Some(&mining_machine_id("pumpjack")));
    assert_close(step.extraction_rate().get(), 6.0);
    assert_close(step.fractional_machine_count().unwrap().get(), 6.0);
    assert_eq!(step.installed_machine_count(), Some(6));
}

#[test]
fn resource_extraction_without_compatible_miners_stays_abstract() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let hard_ore = ResourceCategory::new("hard-ore").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        vec![mining_machine("hard-ore-miner", &hard_ore, 10.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore, 3.0))).unwrap();
    let step = &result.extraction_steps()[0];

    assert!(step.mining_machine().is_none());
    assert!(step.fractional_machine_count().is_none());
    assert!(step.installed_machine_count().is_none());
    assert_close(step.extraction_rate().get(), 3.0);
}

#[test]
fn electric_resource_extraction_reports_power_demand() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = catalog_with_resources_and_miners(
        [ore.clone()],
        vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            2.0,
        )],
        vec![configured_mining_machine(
            "electric-mining-drill",
            &basic_solid,
            0.5,
            90_000.0,
            MachineEnergySource::Electric {
                drain: NonNegative::new(3_000.0).unwrap(),
            },
        )],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore, 9.0))).unwrap();
    let step = &result.extraction_steps()[0];
    let StepEnergy::Electric(power) = step.energy().expect("expected miner power") else {
        panic!("expected electric miner power");
    };

    assert_close(power.fractional_process_watts().get(), 3_348_000.0);
    assert_close(power.installed_full_load_watts().get(), 3_348_000.0);
    let total = result.electric_power().expect("expected electric total");
    assert_close(total.fractional_process_watts().get(), 3_348_000.0);
    assert_close(total.installed_full_load_watts().get(), 3_348_000.0);
}

#[test]
fn burner_resource_extraction_expands_fuel_demand() {
    let ore = item("iron-ore");
    let wood = item("wood");
    let stone = item("stone");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [ore.clone(), wood.clone(), stone]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            2.0,
        )],
        mining_machines: vec![configured_mining_machine(
            "burner-mining-drill",
            &basic_solid,
            0.25,
            150_000.0,
            MachineEnergySource::Burner {
                fuel_categories: [FuelCategory::new("chemical").unwrap()]
                    .into_iter()
                    .collect(),
                effectivity: positive(0.5),
            },
        )],
        fuels: vec![test_fuel("wood", "chemical", 4_000_000.0, Some("stone"))],
        ..CatalogParts::default()
    })
    .unwrap();

    let result = calculate(&catalog, &FactoryPlan::new(target(ore.clone(), 1.0))).unwrap();
    let step = result
        .extraction_steps()
        .iter()
        .find(|step| step.planning_product() == &ore)
        .expect("expected ore extraction step");
    let StepEnergy::Burner(fuel) = step.energy().expect("expected miner fuel") else {
        panic!("expected burner miner fuel");
    };

    assert_eq!(fuel.fuel(), &fuel_id("wood"));
    assert_close(fuel.rate_per_second().get(), 0.6);
    assert_close(rate_for(result.external_inputs(), &wood), 0.6);
    assert_close(result.burner_fuel_demand()[0].rate_per_second().get(), 0.6);
    assert_close(rate_for(result.surplus(), &item("stone")), 0.6);
}

#[test]
fn applies_mining_machine_module_speed_productivity_and_consumption_effects() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let mining = ModuleCategory::new("mining").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [ore.clone()]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            2.0,
        )],
        mining_machines: vec![modular_mining_machine(
            "electric-mining-drill",
            &basic_solid,
            0.5,
            2,
            [
                ModuleEffect::Speed,
                ModuleEffect::Productivity,
                ModuleEffect::Consumption,
            ],
            Some([mining.clone()].into_iter().collect()),
            100.0,
            MachineEnergySource::Electric {
                drain: NonNegative::new(10.0).unwrap(),
            },
        )],
        modules: vec![test_module("combined", "mining", 0.5, 0.25, 0.5)],
        ..CatalogParts::default()
    })
    .unwrap();
    let mut plan = FactoryPlan::new(target(ore.clone(), 12.5));
    plan.set_modules(ore.clone(), [module_id("combined")]);

    let result = calculate(&catalog, &plan).unwrap();
    let step = &result.extraction_steps()[0];
    let StepEnergy::Electric(power) = step.energy().expect("expected miner power") else {
        panic!("expected electric miner power");
    };

    assert_eq!(step.modules(), &[module_id("combined")]);
    assert_close(step.speed_multiplier().get(), 1.5);
    assert_close(step.productivity_effect().get(), 0.25);
    assert_close(step.consumption_multiplier().get(), 1.5);
    assert_close(step.extraction_rate().get(), 10.0);
    assert_close(step.fractional_machine_count().unwrap().get(), 80.0 / 3.0);
    assert_eq!(step.installed_machine_count(), Some(27));
    assert_close(rate_for(step.products(), &ore), 12.5);
    assert_close(power.fractional_process_watts().get(), 4_270.0);
    assert_close(power.installed_full_load_watts().get(), 4_320.0);
}

#[test]
fn mining_productivity_reduces_operations_but_not_fluid_per_operation() {
    let ore = item("uranium-ore");
    let acid = fluid("sulfuric-acid");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let mining = ModuleCategory::new("mining").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [ore.clone(), acid.clone()]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source(
            "uranium-ore",
            ore.clone(),
            1.0,
            Some((acid.clone(), 10.0)),
        )],
        mining_machines: vec![modular_mining_machine(
            "electric-mining-drill",
            &basic_solid,
            1.0,
            1,
            [ModuleEffect::Productivity],
            Some([mining.clone()].into_iter().collect()),
            90_000.0,
            MachineEnergySource::Electric {
                drain: NonNegative::new(0.0).unwrap(),
            },
        )],
        modules: vec![test_module("productivity", "mining", 0.0, 0.25, 0.0)],
        ..CatalogParts::default()
    })
    .unwrap();
    let mut plan = FactoryPlan::new(target(ore.clone(), 10.0));
    plan.set_modules(ore.clone(), [module_id("productivity")]);

    let result = calculate(&catalog, &plan).unwrap();
    let step = &result.extraction_steps()[0];

    assert_close(step.extraction_rate().get(), 8.0);
    assert_close(step.fractional_machine_count().unwrap().get(), 8.0);
    assert_close(rate_for(step.required_fluids(), &acid), 80.0);
    assert_close(rate_for(result.external_inputs(), &acid), 80.0);
    assert_close(rate_for(step.products(), &ore), 10.0);
}

#[test]
fn validates_mining_module_configurations() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let speed_category = ModuleCategory::new("speed").unwrap();
    let productivity_category = ModuleCategory::new("productivity").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [ore.clone()]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        mining_machines: vec![modular_mining_machine(
            "electric-mining-drill",
            &basic_solid,
            1.0,
            1,
            [ModuleEffect::Speed],
            Some([speed_category.clone()].into_iter().collect()),
            90_000.0,
            MachineEnergySource::Electric {
                drain: NonNegative::new(0.0).unwrap(),
            },
        )],
        modules: vec![
            test_module("speed", "speed", 0.2, 0.0, 0.0),
            test_module("productivity", "productivity", 0.0, 0.1, 0.0),
            test_module("future", "speed", 0.2, 0.0, 0.0)
                .with_unsupported_effects(["future-effect".into()]),
        ],
        ..CatalogParts::default()
    })
    .unwrap();
    let mut plan = FactoryPlan::new(target(ore.clone(), 1.0));

    plan.set_modules(ore.clone(), [module_id("speed"), module_id("speed")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::TooManyMiningModules {
            commodity: ore.clone(),
            miner: mining_machine_id("electric-mining-drill"),
            selected: 2,
            slots: 1,
        })
    );

    plan.set_modules(ore.clone(), [module_id("missing")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MissingModuleChoice {
            commodity: ore.clone(),
            module: module_id("missing"),
        })
    );

    plan.set_modules(ore.clone(), [module_id("future")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::UnsupportedModuleChoice {
            commodity: ore.clone(),
            module: module_id("future"),
        })
    );

    plan.set_modules(ore.clone(), [module_id("productivity")]);
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MiningMachineDisallowsModuleCategory {
            commodity: ore,
            miner: mining_machine_id("electric-mining-drill"),
            module: module_id("productivity"),
            category: productivity_category,
        })
    );
}

#[test]
fn validates_mining_module_effect_restrictions() {
    let ore = item("iron-ore");
    let basic_solid = ResourceCategory::new("basic-solid").unwrap();
    let speed_category = ModuleCategory::new("speed").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [ore.clone()]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source_with_category(
            "iron-ore",
            &basic_solid,
            ore.clone(),
            1.0,
            1.0,
        )],
        mining_machines: vec![modular_mining_machine(
            "electric-mining-drill",
            &basic_solid,
            1.0,
            1,
            [],
            Some([speed_category].into_iter().collect()),
            90_000.0,
            MachineEnergySource::Electric {
                drain: NonNegative::new(0.0).unwrap(),
            },
        )],
        modules: vec![test_module("speed", "speed", 0.2, 0.0, 0.0)],
        ..CatalogParts::default()
    })
    .unwrap();
    let mut plan = FactoryPlan::new(target(ore.clone(), 1.0));
    plan.set_modules(ore.clone(), [module_id("speed")]);

    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MiningMachineDisallowsModuleEffect {
            commodity: ore,
            miner: mining_machine_id("electric-mining-drill"),
            module: module_id("speed"),
            effect: ModuleEffect::Speed,
        })
    );
}

#[test]
fn resource_required_fluids_expand_as_dependencies() {
    let ore = item("uranium-ore");
    let acid = fluid("sulfuric-acid");
    let catalog = catalog_with_resources(
        [ore.clone(), acid.clone()],
        vec![],
        vec![],
        vec![resource_source(
            "uranium-ore",
            ore.clone(),
            0.5,
            Some((acid.clone(), 10.0)),
        )],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(ore.clone(), 1.0))).unwrap();

    assert_eq!(result.extraction_steps().len(), 1);
    assert_close(rate_for(result.external_inputs(), &acid), 20.0);
    assert_eq!(result.dependency_trees()[0].children().len(), 1);
    let acid_node = &result.dependency_trees()[0].children()[0];
    assert_eq!(acid_node.commodity(), &acid);
    assert_eq!(acid_node.kind(), DependencyNodeKind::ExternalInput);
    assert_close(acid_node.required_rate().get(), 20.0);
}

#[test]
fn resource_sources_take_priority_over_recipes_for_default_source_selection() {
    let crude_oil = fluid("crude-oil");
    let crude_oil_barrel = item("crude-oil-barrel");
    let crafting = RecipeCategory::new("crafting-with-fluid").unwrap();
    let catalog = catalog_with_resources(
        [crude_oil.clone(), crude_oil_barrel.clone()],
        vec![
            recipe(
                "empty-crude-oil-barrel",
                &crafting,
                1.0,
                vec![(crude_oil_barrel.clone(), 1.0)],
                crude_oil.clone(),
                50.0,
            ),
            recipe(
                "fill-crude-oil-barrel",
                &crafting,
                1.0,
                vec![(crude_oil.clone(), 50.0)],
                crude_oil_barrel,
                1.0,
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
        vec![resource_source("crude-oil", crude_oil.clone(), 10.0, None)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(crude_oil.clone(), 60.0))).unwrap();

    assert!(result.production_steps().is_empty());
    assert!(result.external_inputs().is_empty());
    assert_eq!(result.extraction_steps().len(), 1);
    assert_eq!(
        result.extraction_steps()[0].source(),
        &factorio_planner_tui::catalog::ProductionSource::Resource(resource_id("crude-oil"))
    );
}

#[test]
fn offshore_pump_water_targets_are_not_external_inputs() {
    let water = fluid("water");
    let catalog = catalog_with_fluid_sources(
        [water.clone()],
        vec![],
        vec![],
        vec![],
        vec![offshore_pump_source(
            "offshore-pump",
            water.clone(),
            1_200.0,
        )],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(water.clone(), 60.0))).unwrap();

    assert!(result.production_steps().is_empty());
    assert!(result.external_inputs().is_empty());
    assert_eq!(result.extraction_steps().len(), 1);
    let step = &result.extraction_steps()[0];
    assert_eq!(
        step.source(),
        &factorio_planner_tui::catalog::ProductionSource::Fluid(fluid_source_id("offshore-pump"))
    );
    assert!(step.mining_machine().is_none());
    assert!(step.fractional_machine_count().is_none());
    assert!(step.installed_machine_count().is_none());
    assert_close(step.extraction_rate().get(), 0.05);
    assert_close(rate_for(step.products(), &water), 60.0);
}

#[test]
fn fluid_sources_take_priority_over_recipes_for_default_source_selection() {
    let water = fluid("water");
    let water_barrel = item("water-barrel");
    let crafting = RecipeCategory::new("crafting-with-fluid").unwrap();
    let catalog = catalog_with_fluid_sources(
        [water.clone(), water_barrel.clone()],
        vec![
            recipe(
                "empty-water-barrel",
                &crafting,
                1.0,
                vec![(water_barrel.clone(), 1.0)],
                water.clone(),
                50.0,
            ),
            recipe(
                "fill-water-barrel",
                &crafting,
                1.0,
                vec![(water.clone(), 50.0)],
                water_barrel,
                1.0,
            ),
        ],
        vec![machine("assembler", [crafting], 1.0)],
        vec![],
        vec![offshore_pump_source(
            "offshore-pump",
            water.clone(),
            1_200.0,
        )],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(water.clone(), 60.0))).unwrap();

    assert!(result.production_steps().is_empty());
    assert!(result.external_inputs().is_empty());
    assert_eq!(result.extraction_steps().len(), 1);
    assert_eq!(
        result.extraction_steps()[0].source(),
        &factorio_planner_tui::catalog::ProductionSource::Fluid(fluid_source_id("offshore-pump"))
    );
}

#[test]
fn burner_boiler_steam_expands_water_and_fuel_dependencies() {
    let water = fluid("water");
    let steam = fluid("steam");
    let coal = item("coal");
    let fuel = Fuel::new(
        fuel_id("coal"),
        item_id("coal"),
        FuelCategory::new("chemical").unwrap(),
        positive(8_000_000.0),
        None,
    );
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [water.clone(), steam.clone(), coal.clone()]
            .into_iter()
            .map(|id| Commodity::new(id, None))
            .collect(),
        resource_sources: vec![resource_source("coal", coal.clone(), 1.0, None)],
        fuels: vec![fuel],
        fluid_sources: vec![
            offshore_pump_source("offshore-pump", water.clone(), 1_200.0),
            burner_boiler_source("boiler", water.clone(), steam.clone(), 60.0),
        ],
        ..CatalogParts::default()
    })
    .unwrap();

    let result = calculate(&catalog, &FactoryPlan::new(target(steam.clone(), 30.0))).unwrap();

    assert!(result.external_inputs().is_empty());
    assert_eq!(result.extraction_steps().len(), 3);
    assert_close(rate_for(result.fluid_flows(), &steam), 30.0);
    assert_close(rate_for(result.fluid_flows(), &water), 30.0);
    assert_close(
        result.burner_fuel_demand()[0].rate_per_second().get(),
        0.1125,
    );
    let tree = &result.dependency_trees()[0];
    assert_eq!(tree.commodity(), &steam);
    assert!(
        tree.children()
            .iter()
            .any(|child| child.commodity() == &water)
    );
    assert!(
        tree.children()
            .iter()
            .any(|child| child.commodity() == &coal)
    );
}

#[test]
fn rocket_launch_sources_expand_launched_item_and_rocket_part_dependencies() {
    let ore = item("iron-ore");
    let low_density = item("low-density-structure");
    let satellite = item("satellite");
    let rocket_fuel = item("rocket-fuel");
    let rocket_part = item("rocket-part");
    let science = item("space-science-pack");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let rocket_building = RecipeCategory::new("rocket-building").unwrap();
    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: [
            ore.clone(),
            low_density.clone(),
            satellite.clone(),
            rocket_fuel.clone(),
            rocket_part.clone(),
            science.clone(),
        ]
        .into_iter()
        .map(|id| Commodity::new(id, None))
        .collect(),
        recipes: vec![
            recipe(
                "low-density-structure",
                &crafting,
                1.0,
                vec![(ore.clone(), 2.0)],
                low_density.clone(),
                1.0,
            ),
            recipe(
                "satellite",
                &crafting,
                5.0,
                vec![(low_density.clone(), 100.0)],
                satellite.clone(),
                1.0,
            ),
            recipe(
                "rocket-fuel",
                &crafting,
                1.0,
                vec![(ore.clone(), 10.0)],
                rocket_fuel.clone(),
                1.0,
            ),
            recipe(
                "rocket-part",
                &rocket_building,
                3.0,
                vec![(low_density.clone(), 10.0), (rocket_fuel.clone(), 10.0)],
                rocket_part.clone(),
                1.0,
            ),
        ],
        machines: vec![
            machine("assembler", [crafting], 1.0),
            machine("rocket-silo", [rocket_building], 1.0),
        ],
        rocket_launch_sources: vec![rocket_launch_source(
            "satellite",
            "satellite",
            science.clone(),
            1000.0,
            "rocket-part",
            100.0,
        )],
        ..CatalogParts::default()
    })
    .unwrap();

    let result = calculate(&catalog, &FactoryPlan::new(target(science.clone(), 1.0))).unwrap();

    assert!(
        result
            .external_inputs()
            .iter()
            .all(|input| input.commodity() != &science)
    );
    let launch_step = result
        .extraction_steps()
        .iter()
        .find(|step| step.planning_product() == &science)
        .expect("expected rocket launch step");
    assert_eq!(
        launch_step.source(),
        &factorio_planner_tui::catalog::ProductionSource::RocketLaunch(rocket_launch_id(
            "satellite"
        ))
    );
    assert_close(launch_step.extraction_rate().get(), 0.001);
    assert_close(rate_for(launch_step.required_fluids(), &satellite), 0.001);
    assert_close(rate_for(launch_step.required_fluids(), &rocket_part), 0.1);
    assert_close(
        step_for(&result, &satellite).required_output_rate().get(),
        0.001,
    );
    assert_close(
        step_for(&result, &rocket_part).required_output_rate().get(),
        0.1,
    );
    assert_close(
        step_for(&result, &low_density).required_output_rate().get(),
        1.1,
    );
    assert_close(
        step_for(&result, &rocket_fuel).required_output_rate().get(),
        1.0,
    );

    let tree = &result.dependency_trees()[0];
    assert_eq!(tree.commodity(), &science);
    assert!(
        tree.children()
            .iter()
            .any(|child| child.commodity() == &satellite)
    );
    assert!(
        tree.children()
            .iter()
            .any(|child| child.commodity() == &rocket_part)
    );
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
fn explicit_source_choices_can_select_recipe_or_non_recipe_sources() {
    let ore = item("iron-ore");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog_with_resources(
        [ore.clone()],
        vec![recipe(
            "synthetic-iron-ore",
            &crafting,
            1.0,
            vec![],
            ore.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 1.0)],
        vec![resource_source("iron-ore", ore.clone(), 1.0, None)],
    );

    let implicit = calculate(&catalog, &FactoryPlan::new(target(ore.clone(), 2.0))).unwrap();
    assert!(implicit.production_steps().is_empty());
    assert_eq!(
        implicit.extraction_steps()[0].source(),
        &ProductionSource::Resource(resource_id("iron-ore"))
    );

    let mut plan = FactoryPlan::new(target(ore.clone(), 2.0));
    assert_eq!(
        plan.set_source_choice(
            ore.clone(),
            ProductionSource::Recipe(recipe_id("synthetic-iron-ore")),
        ),
        None
    );
    assert_eq!(
        plan.recipe_choice(&ore),
        Some(&recipe_id("synthetic-iron-ore"))
    );
    let recipe_backed = calculate(&catalog, &plan).unwrap();
    assert_eq!(
        recipe_backed.production_steps()[0].recipe(),
        &recipe_id("synthetic-iron-ore")
    );

    assert_eq!(
        plan.set_source_choice(
            ore.clone(),
            ProductionSource::Resource(resource_id("iron-ore")),
        ),
        Some(ProductionSource::Recipe(recipe_id("synthetic-iron-ore")))
    );
    assert_eq!(plan.recipe_choice(&ore), None);
    let extraction_backed = calculate(&catalog, &plan).unwrap();
    assert!(extraction_backed.production_steps().is_empty());
    assert_eq!(
        extraction_backed.extraction_steps()[0].source(),
        &ProductionSource::Resource(resource_id("iron-ore"))
    );

    assert_eq!(
        plan.clear_source_choice(&ore),
        Some(ProductionSource::Resource(resource_id("iron-ore")))
    );
}

#[test]
fn rejects_missing_and_wrong_product_source_choices() {
    let ore = item("iron-ore");
    let plate = item("iron-plate");
    let catalog = catalog_with_resources(
        [ore.clone(), plate.clone()],
        vec![],
        vec![],
        vec![resource_source("iron-ore", ore.clone(), 1.0, None)],
    );

    let mut missing = FactoryPlan::new(target(ore.clone(), 1.0));
    missing.set_source_choice(
        ore.clone(),
        ProductionSource::Resource(resource_id("missing")),
    );
    assert_eq!(
        calculate(&catalog, &missing),
        Err(PlannerError::MissingSourceChoice {
            commodity: ore.clone(),
            selected_source: ProductionSource::Resource(resource_id("missing")),
        })
    );

    let mut wrong_product = FactoryPlan::new(target(plate.clone(), 1.0));
    wrong_product.set_source_choice(
        plate.clone(),
        ProductionSource::Resource(resource_id("iron-ore")),
    );
    assert_eq!(
        calculate(&catalog, &wrong_product),
        Err(PlannerError::SourceDoesNotProduceCommodity {
            commodity: plate,
            selected_source: ProductionSource::Resource(resource_id("iron-ore")),
        })
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
fn calculates_electric_process_and_installed_power_with_drain_and_modules() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let efficiency = ModuleCategory::new("efficiency").unwrap();
    let plate_recipe = recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0)
        .with_module_policy(
            [ModuleEffect::Consumption],
            Some([efficiency.clone()].into_iter().collect()),
            NonNegative::new(3.0).unwrap(),
        );
    let electric_machine = Machine::new(
        machine_id("assembler"),
        [crafting],
        positive(1.0),
        1,
        [ModuleEffect::Consumption],
        Some([efficiency].into_iter().collect()),
        positive(100.0),
        MachineEnergySource::Electric {
            drain: NonNegative::new(10.0).unwrap(),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [plate.clone()],
        vec![plate_recipe],
        vec![electric_machine],
        vec![test_module("power", "efficiency", 0.0, 0.0, 0.5)],
        vec![],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.5));
    plan.set_modules(plate.clone(), [module_id("power")]);

    let result = calculate(&catalog, &plan).unwrap();
    let step = step_for(&result, &plate);
    let StepEnergy::Electric(power) = step.energy() else {
        panic!("expected electric power");
    };

    assert_close(power.fractional_process_watts().get(), 245.0);
    assert_close(power.installed_full_load_watts().get(), 320.0);
    let total = result.electric_power().expect("expected electric total");
    assert_close(total.fractional_process_watts().get(), 245.0);
    assert_close(total.installed_full_load_watts().get(), 320.0);
    assert!(result.burner_fuel_demand().is_empty());
}

#[test]
fn defaults_to_the_best_compatible_fuel_and_allows_overrides() {
    let plate = item("plate");
    let poor = item("poor-fuel");
    let rich_a = item("a-rich-fuel");
    let rich_z = item("z-rich-fuel");
    let smelting = RecipeCategory::new("smelting").unwrap();
    let burner = Machine::new(
        machine_id("furnace"),
        [smelting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(0.5),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [plate.clone(), poor.clone(), rich_a.clone(), rich_z],
        vec![recipe("plate", &smelting, 1.0, vec![], plate.clone(), 1.0)],
        vec![burner],
        vec![],
        vec![
            test_fuel("poor-fuel", "chemical", 500.0, None),
            test_fuel("z-rich-fuel", "chemical", 2_000.0, None),
            test_fuel("a-rich-fuel", "chemical", 2_000.0, None),
        ],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    let defaulted = calculate(&catalog, &plan).unwrap();
    let StepEnergy::Burner(default_fuel) = step_for(&defaulted, &plate).energy() else {
        panic!("expected burner fuel");
    };
    assert_eq!(default_fuel.fuel(), &fuel_id("a-rich-fuel"));
    assert_eq!(default_fuel.fuel_item(), &item_id("a-rich-fuel"));
    assert_close(default_fuel.rate_per_second().get(), 0.1);
    assert_eq!(defaulted.external_inputs()[0].commodity(), &rich_a);

    assert_eq!(
        plan.set_fuel_choice(plate.clone(), fuel_id("poor-fuel")),
        None
    );
    assert_eq!(plan.fuel_choice(&plate), Some(&fuel_id("poor-fuel")));
    let overridden = calculate(&catalog, &plan).unwrap();
    let StepEnergy::Burner(overridden_fuel) = step_for(&overridden, &plate).energy() else {
        panic!("expected burner fuel");
    };
    assert_eq!(overridden_fuel.fuel(), &fuel_id("poor-fuel"));
    assert_close(overridden_fuel.rate_per_second().get(), 0.4);
    assert_eq!(overridden.external_inputs()[0].commodity(), &poor);
    assert_eq!(plan.clear_fuel_choice(&plate), Some(fuel_id("poor-fuel")));
}

#[test]
fn recursively_produces_burner_fuel_and_reports_burnt_result_surplus() {
    let ore = item("ore");
    let coal = item("coal");
    let ash = item("ash");
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let smelting = RecipeCategory::new("smelting").unwrap();
    let electric = Machine::new(
        machine_id("assembler"),
        [crafting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(50.0),
        MachineEnergySource::Electric {
            drain: NonNegative::new(0.0).unwrap(),
        },
    )
    .unwrap();
    let burner = Machine::new(
        machine_id("furnace"),
        [smelting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(0.5),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [ore.clone(), coal.clone(), ash.clone(), plate.clone()],
        vec![
            recipe(
                "coal",
                &crafting,
                1.0,
                vec![(ore.clone(), 1.0)],
                coal.clone(),
                2.0,
            ),
            recipe("plate", &smelting, 1.0, vec![], plate.clone(), 1.0),
        ],
        vec![electric, burner],
        vec![],
        vec![test_fuel("coal", "chemical", 1_000.0, Some("ash"))],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 2.0))).unwrap();

    let fuel = result
        .burner_fuel_demand()
        .first()
        .expect("expected aggregate burner fuel");
    assert_eq!(fuel.fuel(), &fuel_id("coal"));
    assert_close(fuel.rate_per_second().get(), 0.4);
    assert_close(
        fuel.burnt_result()
            .expect("expected burnt result")
            .rate()
            .get(),
        0.4,
    );
    assert_close(step_for(&result, &coal).required_output_rate().get(), 0.4);
    assert_eq!(result.external_inputs()[0].commodity(), &ore);
    assert_close(result.external_inputs()[0].rate().get(), 0.2);
    assert_close(rate_for(result.surplus(), &ash), 0.4);
}

#[test]
fn validates_explicit_fuel_choices() {
    let plate = item("plate");
    let coal = item("coal");
    let biofuel = item("biofuel");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let burner = Machine::new(
        machine_id("furnace"),
        [crafting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(1.0),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [plate.clone(), coal, biofuel],
        vec![recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0)],
        vec![burner],
        vec![],
        vec![test_fuel("biofuel", "biological", 1_000.0, None)],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));

    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::NoCompatibleFuel {
            commodity: plate.clone(),
            machine: machine_id("furnace"),
        })
    );

    plan.set_fuel_choice(plate.clone(), fuel_id("missing"));
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MissingFuelChoice {
            commodity: plate.clone(),
            fuel: fuel_id("missing"),
        })
    );

    plan.set_fuel_choice(plate.clone(), fuel_id("biofuel"));
    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::IncompatibleFuelChoice {
            commodity: plate,
            machine: machine_id("furnace"),
            fuel: fuel_id("biofuel"),
            category: FuelCategory::new("biological").unwrap(),
        })
    );
}

#[test]
fn rejects_fuel_choices_for_electric_machines() {
    let plate = item("plate");
    let coal = item("coal");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog_with_energy(
        [plate.clone(), coal],
        vec![recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0)],
        vec![machine("assembler", [crafting], 1.0)],
        vec![],
        vec![test_fuel("coal", "chemical", 1_000.0, None)],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 1.0));
    plan.set_fuel_choice(plate.clone(), fuel_id("coal"));

    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::FuelChoiceForNonBurnerMachine {
            commodity: plate,
            machine: machine_id("assembler"),
            fuel: fuel_id("coal"),
        })
    );
}

#[test]
fn detects_burner_fuel_cycles_and_resolves_them_with_external_fuel() {
    let plate = item("plate");
    let coal = item("coal");
    let smelting = RecipeCategory::new("smelting").unwrap();
    let fuel_processing = RecipeCategory::new("fuel-processing").unwrap();
    let plate_burner = Machine::new(
        machine_id("plate-furnace"),
        [smelting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(1.0),
        },
    )
    .unwrap();
    let coal_burner = Machine::new(
        machine_id("coal-processor"),
        [fuel_processing.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("metal").unwrap()].into_iter().collect(),
            effectivity: positive(1.0),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [plate.clone(), coal.clone()],
        vec![
            recipe("plate", &smelting, 1.0, vec![], plate.clone(), 1.0),
            recipe("coal", &fuel_processing, 1.0, vec![], coal.clone(), 1.0),
        ],
        vec![plate_burner, coal_burner],
        vec![],
        vec![
            test_fuel("coal", "chemical", 1_000.0, None),
            test_fuel("plate", "metal", 1_000.0, None),
        ],
    );

    assert_eq!(
        calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 1.0))),
        Err(PlannerError::Cycle {
            path: vec![plate.clone(), coal.clone(), plate.clone()],
        })
    );

    let external =
        FactoryPlan::new(target(plate.clone(), 1.0)).with_external_inputs([coal.clone()]);
    let result = calculate(&catalog, &external).unwrap();
    assert_eq!(result.production_steps().len(), 1);
    assert_eq!(result.external_inputs()[0].commodity(), &coal);
}

#[test]
fn skips_unsupported_default_energy_sources_and_rejects_explicit_ones() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let plate_recipe = recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0);
    let heat_machine = Machine::new(
        machine_id("heat-fast"),
        [crafting.clone()],
        positive(2.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Unsupported(UnsupportedEnergySource::Heat),
    )
    .unwrap();
    let electric_machine = machine("electric-slow", [crafting.clone()], 1.0);
    let mixed_catalog = catalog(
        [plate.clone()],
        vec![plate_recipe.clone()],
        vec![heat_machine, electric_machine],
    );

    let defaulted = calculate(
        &mixed_catalog,
        &FactoryPlan::new(target(plate.clone(), 1.0)),
    )
    .unwrap();
    assert_eq!(
        step_for(&defaulted, &plate).machine(),
        &machine_id("electric-slow")
    );

    let mut explicit = FactoryPlan::new(target(plate.clone(), 1.0));
    explicit.set_machine_choice(recipe_id("plate"), machine_id("heat-fast"));
    assert_eq!(
        calculate(&mixed_catalog, &explicit),
        Err(PlannerError::UnsupportedMachineEnergySource {
            recipe: recipe_id("plate"),
            machine: machine_id("heat-fast"),
            energy_source: UnsupportedEnergySource::Heat,
        })
    );

    let fluid_machine = Machine::new(
        machine_id("fluid-only"),
        [crafting],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Unsupported(UnsupportedEnergySource::Fluid),
    )
    .unwrap();
    let fluid_catalog = catalog([plate.clone()], vec![plate_recipe], vec![fluid_machine]);
    assert_eq!(
        calculate(
            &fluid_catalog,
            &FactoryPlan::new(target(plate.clone(), 1.0))
        ),
        Err(PlannerError::UnsupportedMachineEnergySource {
            recipe: recipe_id("plate"),
            machine: machine_id("fluid-only"),
            energy_source: UnsupportedEnergySource::Fluid,
        })
    );
}

#[test]
fn summarizes_item_and_fluid_flows_without_belt_equivalents_when_no_belt_is_selected() {
    let plate = item("plate");
    let water = fluid("water");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone(), water.clone()],
        vec![recipe(
            "plate",
            &crafting,
            1.0,
            vec![(water.clone(), 2.0)],
            plate.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 1.0)],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 3.0))).unwrap();

    assert_eq!(result.item_flows().len(), 1);
    assert_eq!(result.item_flows()[0].commodity(), &plate);
    assert_close(rate_for(result.item_flows(), &plate), 3.0);
    assert_eq!(result.fluid_flows().len(), 1);
    assert_eq!(result.fluid_flows()[0].commodity(), &water);
    assert_close(rate_for(result.fluid_flows(), &water), 6.0);
    assert!(result.belt_equivalents().is_empty());
}

#[test]
fn selected_belt_reports_exact_and_rounded_equivalents_for_multiple_item_flows() {
    let ore = item("ore");
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog_with_belts(
        [ore.clone(), plate.clone()],
        vec![recipe(
            "plate",
            &crafting,
            1.0,
            vec![(ore.clone(), 1.0)],
            plate.clone(),
            1.0,
        )],
        vec![machine("assembler", [crafting], 1.0)],
        vec![belt("transport-belt", 15.0)],
    );
    let plan =
        FactoryPlan::new(target(plate.clone(), 22.5)).with_selected_belt(belt_id("transport-belt"));

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(
        result
            .belt_equivalents()
            .iter()
            .map(factorio_planner_tui::planner::BeltEquivalent::commodity)
            .collect::<Vec<_>>(),
        [&ore, &plate]
    );
    for commodity in [&ore, &plate] {
        let equivalent = belt_equivalent_for(result.belt_equivalents(), commodity);
        assert_eq!(equivalent.belt(), &belt_id("transport-belt"));
        assert_close(equivalent.rate().get(), 22.5);
        assert_close(equivalent.exact_belts().get(), 1.5);
        assert_eq!(equivalent.installed_belts(), 2);
    }
}

#[test]
fn dependency_trees_show_targets_external_boundaries_and_shared_intermediates() {
    let (catalog, ore, plate, gear, pipe) = shared_intermediate_catalog();
    let mut plan = FactoryPlan::new(target(gear.clone(), 3.0));
    plan.add_target(target(pipe.clone(), 4.0));

    let result = calculate(&catalog, &plan).unwrap();

    assert_eq!(result.dependency_trees().len(), 2);
    assert_eq!(result.dependency_trees()[0].commodity(), &gear);
    assert_eq!(
        result.dependency_trees()[0].kind(),
        DependencyNodeKind::Production
    );
    assert!(!result.dependency_trees()[0].is_shared());
    let gear_plate = &result.dependency_trees()[0].children()[0];
    assert_eq!(gear_plate.commodity(), &plate);
    assert_eq!(gear_plate.kind(), DependencyNodeKind::Production);
    assert!(gear_plate.is_shared());
    assert_close(gear_plate.required_rate().get(), 6.0);
    assert_eq!(gear_plate.children()[0].commodity(), &ore);
    assert_eq!(
        gear_plate.children()[0].kind(),
        DependencyNodeKind::ExternalInput
    );

    assert_eq!(result.dependency_trees()[1].commodity(), &pipe);
    let pipe_plate = &result.dependency_trees()[1].children()[0];
    assert_eq!(pipe_plate.commodity(), &plate);
    assert!(pipe_plate.is_shared());
    assert_close(pipe_plate.required_rate().get(), 4.0);
}

#[test]
fn dependency_tree_includes_recursive_burner_fuel_dependencies() {
    let coal = item("coal");
    let ash = item("ash");
    let plate = item("iron-plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let burner = Machine::new(
        machine_id("burner-assembler"),
        [crafting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(8.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(0.5),
        },
    )
    .unwrap();
    let electric = machine("electric-assembler", [crafting.clone()], 1.0);
    let catalog = catalog_with_energy(
        [coal.clone(), ash, plate.clone()],
        vec![
            recipe("coal", &crafting, 1.0, vec![], coal.clone(), 1.0),
            recipe("iron-plate", &crafting, 1.0, vec![], plate.clone(), 1.0),
        ],
        vec![burner, electric],
        vec![],
        vec![test_fuel("coal", "chemical", 4.0, None)],
    );
    let mut plan = FactoryPlan::new(target(plate.clone(), 2.0));
    plan.set_machine_choice(recipe_id("iron-plate"), machine_id("burner-assembler"));
    plan.set_machine_choice(recipe_id("coal"), machine_id("electric-assembler"));

    let result = calculate(&catalog, &plan).unwrap();

    let fuel_node = result.dependency_trees()[0]
        .children()
        .iter()
        .find(|node| node.commodity() == &coal)
        .expect("expected burner fuel dependency");
    assert_eq!(fuel_node.kind(), DependencyNodeKind::FuelInput);
    assert_eq!(fuel_node.recipe(), Some(&recipe_id("coal")));
    assert_eq!(fuel_node.machine(), Some(&machine_id("electric-assembler")));
    assert_close(fuel_node.required_rate().get(), 8.0);
}

#[test]
fn rejects_missing_selected_belt() {
    let plate = item("plate");
    let crafting = RecipeCategory::new("crafting").unwrap();
    let catalog = catalog(
        [plate.clone()],
        vec![recipe("plate", &crafting, 1.0, vec![], plate.clone(), 1.0)],
        vec![machine("assembler", [crafting], 1.0)],
    );
    let plan = FactoryPlan::new(target(plate, 1.0)).with_selected_belt(belt_id("missing"));

    assert_eq!(
        calculate(&catalog, &plan),
        Err(PlannerError::MissingBeltChoice {
            belt: belt_id("missing"),
        })
    );
}

#[test]
fn selected_belt_does_not_create_fluid_capacity_equivalents() {
    let water = fluid("water");
    let pumping = RecipeCategory::new("pumping").unwrap();
    let catalog = catalog_with_belts(
        [water.clone()],
        vec![recipe("water", &pumping, 1.0, vec![], water.clone(), 30.0)],
        vec![machine("pump", [pumping], 1.0)],
        vec![belt("transport-belt", 15.0)],
    );
    let plan =
        FactoryPlan::new(target(water.clone(), 30.0)).with_selected_belt(belt_id("transport-belt"));

    let result = calculate(&catalog, &plan).unwrap();

    assert!(result.item_flows().is_empty());
    assert_eq!(result.fluid_flows().len(), 1);
    assert_close(rate_for(result.fluid_flows(), &water), 30.0);
    assert!(result.belt_equivalents().is_empty());
}

#[test]
fn burnt_results_are_included_once_in_item_flows() {
    let coal = item("coal");
    let ash = item("ash");
    let plate = item("plate");
    let smelting = RecipeCategory::new("smelting").unwrap();
    let burner = Machine::new(
        machine_id("furnace"),
        [smelting.clone()],
        positive(1.0),
        0,
        [],
        None,
        positive(100.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(1.0),
        },
    )
    .unwrap();
    let catalog = catalog_with_energy(
        [coal.clone(), ash.clone(), plate.clone()],
        vec![recipe("plate", &smelting, 1.0, vec![], plate.clone(), 1.0)],
        vec![burner],
        vec![],
        vec![test_fuel("coal", "chemical", 1_000.0, Some("ash"))],
    );

    let result = calculate(&catalog, &FactoryPlan::new(target(plate.clone(), 2.0))).unwrap();

    assert_eq!(result.item_flows().len(), 3);
    assert_close(rate_for(result.item_flows(), &plate), 2.0);
    assert_close(rate_for(result.item_flows(), &coal), 0.2);
    assert_close(rate_for(result.item_flows(), &ash), 0.2);
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
