use factorio_planner_tui::catalog::{
    Belt, BeltId, Catalog, CatalogError, CatalogParts, Commodity, CommodityId, DatasetFingerprint,
    Finite, FluidId, Fuel, FuelCategory, FuelId, Ingredient, ItemId, Machine, MachineEnergySource,
    MachineId, Module, ModuleCategory, ModuleEffect, ModuleId, NonNegative, NumericError, Positive,
    Product, Recipe, RecipeCategory, RecipeId, RecordError,
};

fn item(name: &str) -> ItemId {
    ItemId::new(name).expect("test item ID should be valid")
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

fn electric_machine(
    name: &str,
    categories: impl IntoIterator<Item = RecipeCategory>,
    crafting_speed: f64,
) -> Result<Machine, RecordError> {
    Machine::new(
        machine_id(name),
        categories,
        positive(crafting_speed),
        0,
        [],
        None,
        positive(90_000.0),
        MachineEnergySource::Electric {
            drain: NonNegative::new(3_000.0).unwrap(),
        },
    )
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < f64::EPSILON,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn typed_ids_preserve_kind_and_text() {
    let item_id = ItemId::new("water").unwrap();
    let fluid_id = FluidId::new("water").unwrap();

    assert_eq!(item_id.as_str(), "water");
    assert_eq!(item_id.to_string(), "water");
    assert_eq!(CommodityId::from(item_id), CommodityId::Item(item("water")));
    assert_eq!(
        CommodityId::from(fluid_id.clone()),
        CommodityId::Fluid(fluid_id)
    );
    assert_ne!(
        CommodityId::Item(item("water")),
        CommodityId::Fluid(FluidId::new("water").unwrap())
    );
    assert_eq!(
        DatasetFingerprint::new("abc123").unwrap().as_str(),
        "abc123"
    );
}

#[test]
fn typed_ids_reject_empty_text() {
    assert!(ItemId::new("").is_err());
    assert!(RecipeId::new("").is_err());
}

#[test]
fn numeric_wrappers_reject_invalid_values() {
    assert_eq!(Positive::new(0.0), Err(NumericError::NotPositive(0.0)));
    assert_eq!(NonNegative::new(-1.0), Err(NumericError::Negative(-1.0)));
    assert!(matches!(
        Finite::new(f64::INFINITY),
        Err(NumericError::NotFinite(_))
    ));
    assert!(matches!(
        Positive::new(f64::NAN),
        Err(NumericError::NotFinite(_))
    ));
}

#[test]
fn numeric_wrappers_expose_valid_values() {
    assert_close(positive(2.5).get(), 2.5);
    assert_close(NonNegative::new(0.0).unwrap().get(), 0.0);
    assert_close(Finite::new(-0.25).unwrap().get(), -0.25);
}

#[test]
fn records_validate_intrinsic_invariants() {
    let plate = CommodityId::Item(item("iron-plate"));
    let gear = CommodityId::Item(item("iron-gear-wheel"));
    let ingredient = Ingredient::new(plate.clone(), positive(2.0));
    let product = Product::new(gear.clone(), positive(1.0));

    let recipe = Recipe::new(
        recipe_id("iron-gear-wheel"),
        RecipeCategory::new("crafting").unwrap(),
        positive(0.5),
        vec![ingredient],
        vec![product],
        Some(gear),
        true,
    )
    .unwrap();

    assert_eq!(recipe.id().as_str(), "iron-gear-wheel");
    assert_eq!(recipe.category().as_str(), "crafting");
    assert!(recipe.visible());
    assert_close(recipe.duration().get(), 0.5);
    assert_eq!(recipe.ingredients()[0].commodity(), &plate);
    assert_close(recipe.ingredients()[0].amount().get(), 2.0);
    assert_close(recipe.products()[0].amount().get(), 1.0);
    assert_eq!(
        recipe.main_product(),
        Some(&CommodityId::Item(item("iron-gear-wheel")))
    );

    assert_eq!(
        Recipe::new(
            recipe_id("empty"),
            RecipeCategory::new("crafting").unwrap(),
            positive(1.0),
            vec![],
            vec![],
            None,
            true,
        ),
        Err(RecordError::RecipeHasNoProducts {
            recipe: recipe_id("empty")
        })
    );

    assert_eq!(
        Recipe::new(
            recipe_id("wrong-main-product"),
            RecipeCategory::new("crafting").unwrap(),
            positive(1.0),
            vec![],
            vec![Product::new(plate.clone(), positive(1.0))],
            Some(CommodityId::Item(item("copper-plate"))),
            true,
        ),
        Err(RecordError::MainProductNotProduced {
            recipe: recipe_id("wrong-main-product"),
            commodity: CommodityId::Item(item("copper-plate")),
        })
    );

    assert_eq!(
        electric_machine("assembler", [], 1.0),
        Err(RecordError::MachineHasNoCraftingCategories {
            machine: machine_id("assembler")
        })
    );
}

#[test]
fn machine_module_fuel_and_belt_records_expose_validated_data() {
    let crafting = RecipeCategory::new("crafting").unwrap();
    let speed_modules = ModuleCategory::new("speed").unwrap();
    let machine = Machine::new(
        machine_id("assembler"),
        [crafting.clone()],
        positive(1.25),
        2,
        [ModuleEffect::Speed, ModuleEffect::Consumption],
        Some([speed_modules.clone()].into_iter().collect()),
        positive(90_000.0),
        MachineEnergySource::Burner {
            fuel_categories: [FuelCategory::new("chemical").unwrap()]
                .into_iter()
                .collect(),
            effectivity: positive(0.8),
        },
    )
    .unwrap();
    assert_eq!(machine.id(), &machine_id("assembler"));
    assert_eq!(machine.crafting_categories().iter().next(), Some(&crafting));
    assert_close(machine.crafting_speed().get(), 1.25);
    assert_eq!(machine.module_slots(), 2);
    assert_eq!(
        machine.allowed_effects(),
        &[ModuleEffect::Speed, ModuleEffect::Consumption]
            .into_iter()
            .collect()
    );
    assert_eq!(
        machine.allowed_module_categories(),
        Some(&[speed_modules].into_iter().collect())
    );
    assert_close(machine.energy_usage().get(), 90_000.0);
    assert!(machine.supports_category(&crafting));
    assert_close(machine.crafts_per_second(positive(0.5)), 2.5);
    assert!(matches!(
        machine.energy_source(),
        MachineEnergySource::Burner { effectivity, .. }
            if (effectivity.get() - 0.8).abs() < f64::EPSILON
    ));

    let module = Module::new(
        ModuleId::new("productivity-module").unwrap(),
        ModuleCategory::new("productivity").unwrap(),
        Finite::new(-0.15).unwrap(),
        Finite::new(0.04).unwrap(),
        Finite::new(0.4).unwrap(),
    );
    assert_eq!(module.id().as_str(), "productivity-module");
    assert_eq!(module.category().as_str(), "productivity");
    assert_close(module.speed_effect().get(), -0.15);
    assert_close(module.productivity_effect().get(), 0.04);
    assert_close(module.consumption_effect().get(), 0.4);

    let fuel = Fuel::new(
        FuelId::new("wood").unwrap(),
        item("wood"),
        FuelCategory::new("chemical").unwrap(),
        positive(2_000_000.0),
        Some(item("ash")),
    );
    assert_eq!(fuel.id().as_str(), "wood");
    assert_eq!(fuel.item(), &item("wood"));
    assert_eq!(fuel.category().as_str(), "chemical");
    assert_close(fuel.fuel_value().get(), 2_000_000.0);
    assert_eq!(fuel.burnt_result(), Some(&item("ash")));

    let belt = Belt::new(BeltId::new("fast-transport-belt").unwrap(), positive(30.0));
    assert_eq!(belt.id().as_str(), "fast-transport-belt");
    assert_close(belt.throughput().get(), 30.0);
}

#[test]
fn catalog_indexes_and_looks_up_records_deterministically() {
    let plate = CommodityId::Item(item("iron-plate"));
    let gear = CommodityId::Item(item("iron-gear-wheel"));
    let coal = CommodityId::Item(item("coal"));
    let crafting = RecipeCategory::new("crafting").unwrap();

    let gear_recipe = Recipe::new(
        recipe_id("z-gear"),
        crafting.clone(),
        positive(0.5),
        vec![Ingredient::new(plate.clone(), positive(2.0))],
        vec![Product::new(gear.clone(), positive(1.0))],
        Some(gear.clone()),
        true,
    )
    .unwrap();
    let alternate_recipe = Recipe::new(
        recipe_id("a-gear"),
        crafting.clone(),
        positive(1.0),
        vec![Ingredient::new(plate.clone(), positive(3.0))],
        vec![Product::new(gear.clone(), positive(1.0))],
        Some(gear.clone()),
        true,
    )
    .unwrap();
    let fast_machine = electric_machine("z-fast", [crafting.clone()], 2.0).unwrap();
    let slow_machine = electric_machine("a-slow", [crafting.clone()], 1.0).unwrap();

    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities: vec![
            Commodity::new(gear.clone(), Some("Iron gear wheel".into())),
            Commodity::new(plate.clone(), None),
            Commodity::new(coal, None),
        ],
        recipes: vec![gear_recipe, alternate_recipe],
        machines: vec![fast_machine, slow_machine],
        modules: vec![Module::new(
            ModuleId::new("speed-module").unwrap(),
            ModuleCategory::new("speed").unwrap(),
            Finite::new(0.2).unwrap(),
            Finite::new(0.0).unwrap(),
            Finite::new(0.5).unwrap(),
        )],
        fuels: vec![Fuel::new(
            FuelId::new("coal").unwrap(),
            item("coal"),
            FuelCategory::new("chemical").unwrap(),
            positive(4_000_000.0),
            None,
        )],
        belts: vec![Belt::new(
            BeltId::new("transport-belt").unwrap(),
            positive(15.0),
        )],
    })
    .unwrap();

    assert_eq!(
        catalog.commodity(&gear).unwrap().localized_name(),
        Some("Iron gear wheel")
    );
    assert_eq!(
        catalog.commodity(&gear).unwrap().display_name(),
        "Iron gear wheel"
    );
    assert_eq!(
        catalog.recipes_for_product(&gear),
        &[recipe_id("a-gear"), recipe_id("z-gear")]
    );
    assert_eq!(
        catalog.machines_for_category(&crafting),
        &[machine_id("a-slow"), machine_id("z-fast")]
    );
    assert!(
        catalog
            .module(&ModuleId::new("speed-module").unwrap())
            .is_some()
    );
    assert!(catalog.fuel(&FuelId::new("coal").unwrap()).is_some());
    assert!(
        catalog
            .belt(&BeltId::new("transport-belt").unwrap())
            .is_some()
    );
}

#[test]
fn catalog_rejects_duplicate_ids() {
    let plate = CommodityId::Item(item("iron-plate"));
    let result = Catalog::try_from_parts(CatalogParts {
        commodities: vec![
            Commodity::new(plate.clone(), None),
            Commodity::new(plate.clone(), Some("duplicate".into())),
        ],
        ..CatalogParts::default()
    });

    assert_eq!(result, Err(CatalogError::DuplicateCommodity { id: plate }));
}

#[test]
fn catalog_rejects_duplicate_record_ids() {
    let product = CommodityId::Item(item("product"));
    let recipe = Recipe::new(
        recipe_id("recipe"),
        RecipeCategory::new("crafting").unwrap(),
        positive(1.0),
        vec![],
        vec![Product::new(product, positive(1.0))],
        None,
        true,
    )
    .unwrap();
    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            recipes: vec![recipe.clone(), recipe],
            ..CatalogParts::default()
        }),
        Err(CatalogError::DuplicateRecipe {
            id: recipe_id("recipe")
        })
    );

    let machine =
        electric_machine("assembler", [RecipeCategory::new("crafting").unwrap()], 1.0).unwrap();
    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            machines: vec![machine.clone(), machine],
            ..CatalogParts::default()
        }),
        Err(CatalogError::DuplicateMachine {
            id: machine_id("assembler")
        })
    );

    let module = Module::new(
        ModuleId::new("speed-module").unwrap(),
        ModuleCategory::new("speed").unwrap(),
        Finite::new(0.2).unwrap(),
        Finite::new(0.0).unwrap(),
        Finite::new(0.5).unwrap(),
    );
    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            modules: vec![module.clone(), module],
            ..CatalogParts::default()
        }),
        Err(CatalogError::DuplicateModule {
            id: ModuleId::new("speed-module").unwrap()
        })
    );

    let fuel = Fuel::new(
        FuelId::new("coal").unwrap(),
        item("coal"),
        FuelCategory::new("chemical").unwrap(),
        positive(4_000_000.0),
        None,
    );
    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            fuels: vec![fuel.clone(), fuel],
            ..CatalogParts::default()
        }),
        Err(CatalogError::DuplicateFuel {
            id: FuelId::new("coal").unwrap()
        })
    );

    let belt = Belt::new(BeltId::new("transport-belt").unwrap(), positive(15.0));
    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            belts: vec![belt.clone(), belt],
            ..CatalogParts::default()
        }),
        Err(CatalogError::DuplicateBelt {
            id: BeltId::new("transport-belt").unwrap()
        })
    );
}

#[test]
fn catalog_rejects_broken_recipe_references() {
    let missing = CommodityId::Item(item("missing"));
    let gear = CommodityId::Item(item("iron-gear-wheel"));
    let recipe = Recipe::new(
        recipe_id("iron-gear-wheel"),
        RecipeCategory::new("crafting").unwrap(),
        positive(0.5),
        vec![Ingredient::new(missing.clone(), positive(2.0))],
        vec![Product::new(gear.clone(), positive(1.0))],
        Some(gear.clone()),
        true,
    )
    .unwrap();

    let result = Catalog::try_from_parts(CatalogParts {
        commodities: vec![Commodity::new(gear, None)],
        recipes: vec![recipe],
        ..CatalogParts::default()
    });

    assert_eq!(
        result,
        Err(CatalogError::MissingRecipeIngredient {
            recipe: recipe_id("iron-gear-wheel"),
            commodity: missing,
        })
    );
}

#[test]
fn catalog_rejects_missing_recipe_products() {
    let missing = CommodityId::Fluid(FluidId::new("steam").unwrap());
    let recipe = Recipe::new(
        recipe_id("steam"),
        RecipeCategory::new("chemistry").unwrap(),
        positive(1.0),
        vec![],
        vec![Product::new(missing.clone(), positive(1.0))],
        Some(missing.clone()),
        true,
    )
    .unwrap();

    assert_eq!(
        Catalog::try_from_parts(CatalogParts {
            recipes: vec![recipe],
            ..CatalogParts::default()
        }),
        Err(CatalogError::MissingRecipeProduct {
            recipe: recipe_id("steam"),
            commodity: missing,
        })
    );
}

#[test]
fn catalog_rejects_broken_fuel_references() {
    let result = Catalog::try_from_parts(CatalogParts {
        fuels: vec![Fuel::new(
            FuelId::new("coal").unwrap(),
            item("coal"),
            FuelCategory::new("chemical").unwrap(),
            positive(4_000_000.0),
            Some(item("ash")),
        )],
        ..CatalogParts::default()
    });

    assert_eq!(
        result,
        Err(CatalogError::MissingFuelItem {
            fuel: FuelId::new("coal").unwrap(),
            item: item("coal"),
        })
    );
}

#[test]
fn catalog_rejects_missing_fuel_burnt_results() {
    let coal = CommodityId::Item(item("coal"));
    let result = Catalog::try_from_parts(CatalogParts {
        commodities: vec![Commodity::new(coal, None)],
        fuels: vec![Fuel::new(
            FuelId::new("coal").unwrap(),
            item("coal"),
            FuelCategory::new("chemical").unwrap(),
            positive(4_000_000.0),
            Some(item("ash")),
        )],
        ..CatalogParts::default()
    });

    assert_eq!(
        result,
        Err(CatalogError::MissingFuelBurntResult {
            fuel: FuelId::new("coal").unwrap(),
            item: item("ash"),
        })
    );
}
