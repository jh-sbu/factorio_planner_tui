use std::io::Cursor;

use factorio_planner_tui::catalog::{
    BeltId, CommodityId, FluidId, FuelCategory, FuelId, ItemId, MachineEnergySource, MachineId,
    ModuleCategory, ModuleEffect, ModuleId, Positive, RecipeCategory, RecipeId,
    UnsupportedEnergySource,
};
use factorio_planner_tui::import::{
    DiagnosticSeverity, ImportError, PrototypeDisposition, parse_data_raw,
};

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
}

fn fluid(name: &str) -> CommodityId {
    CommodityId::Fluid(FluidId::new(name).expect("test fluid ID should be valid"))
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < f64::EPSILON,
        "expected {expected}, got {actual}"
    );
}

fn import(json: &str) -> Result<factorio_planner_tui::import::ImportReport, ImportError> {
    parse_data_raw(Cursor::new(json))
}

fn invalid_data(json: &str) -> Vec<factorio_planner_tui::import::ImportDiagnostic> {
    match import(json) {
        Err(ImportError::InvalidData { diagnostics }) => diagnostics,
        other => panic!("expected invalid import data, got {other:?}"),
    }
}

#[test]
fn imports_modules_fuels_and_transport_belts() {
    let report = import(include_str!("fixtures/modules-fuels-belts-data-raw.json")).unwrap();
    let catalog = report.catalog();

    assert!(report.diagnostics().is_empty());
    assert_eq!(catalog.modules().len(), 2);
    assert_eq!(catalog.fuels().len(), 2);
    assert_eq!(catalog.belts().len(), 2);

    let productivity = catalog
        .module(&ModuleId::new("productivity-module").unwrap())
        .unwrap();
    assert_eq!(
        productivity.category(),
        &ModuleCategory::new("productivity").unwrap()
    );
    assert_close(productivity.speed_effect().get(), -0.15);
    assert_close(productivity.productivity_effect().get(), 0.04);
    assert_close(productivity.consumption_effect().get(), 0.4);
    assert!(productivity.unsupported_effects().is_empty());
    assert!(productivity.is_selectable());

    let speed = catalog
        .module(&ModuleId::new("speed-module").unwrap())
        .unwrap();
    assert_close(speed.speed_effect().get(), 0.2);
    assert_close(speed.productivity_effect().get(), 0.0);
    assert_close(speed.consumption_effect().get(), 0.5);
    assert!(
        catalog.commodity(&item("speed-module")).is_some(),
        "module prototypes must also be item commodities"
    );

    let coal = catalog.fuel(&FuelId::new("coal").unwrap()).unwrap();
    assert_eq!(coal.item(), &ItemId::new("coal").unwrap());
    assert_eq!(coal.category(), &FuelCategory::new("chemical").unwrap());
    assert_close(coal.fuel_value().get(), 8_000_000.0);
    assert_eq!(coal.burnt_result(), Some(&ItemId::new("ash").unwrap()));
    assert!(catalog.fuel(&FuelId::new("inert-item").unwrap()).is_none());

    assert_close(
        catalog
            .belt(&BeltId::new("transport-belt").unwrap())
            .unwrap()
            .throughput()
            .get(),
        15.0,
    );
    assert_close(
        catalog
            .belt(&BeltId::new("fast-transport-belt").unwrap())
            .unwrap()
            .throughput()
            .get(),
        30.0,
    );
}

#[test]
fn converts_energy_units_for_power_and_fuel_values() {
    let report = import(
        r#"{
            "item": {
                "joule-fuel": {
                    "type": "item",
                    "name": "joule-fuel",
                    "fuel_category": "chemical",
                    "fuel_value": "60J"
                },
                "watt-fuel": {
                    "type": "item",
                    "name": "watt-fuel",
                    "fuel_category": "chemical",
                    "fuel_value": "60W"
                }
            },
            "assembling-machine": {
                "joule-machine": {
                    "type": "assembling-machine",
                    "name": "joule-machine",
                    "crafting_categories": ["crafting"],
                    "crafting_speed": 1,
                    "energy_usage": "1J",
                    "energy_source": {"type": "electric"}
                }
            }
        }"#,
    )
    .unwrap();

    assert_close(
        report
            .catalog()
            .fuel(&FuelId::new("joule-fuel").unwrap())
            .unwrap()
            .fuel_value()
            .get(),
        60.0,
    );
    assert_close(
        report
            .catalog()
            .fuel(&FuelId::new("watt-fuel").unwrap())
            .unwrap()
            .fuel_value()
            .get(),
        1.0,
    );
    assert_close(
        report
            .catalog()
            .machine(&MachineId::new("joule-machine").unwrap())
            .unwrap()
            .energy_usage()
            .get(),
        60.0,
    );
}

#[test]
fn retains_unsupported_module_effects_as_selection_blocking_warnings() {
    let report = import(
        r#"{
            "module": {
                "future-module": {
                    "type": "module",
                    "name": "future-module",
                    "category": "future",
                    "effect": {
                        "speed": 0.1,
                        "pollution": -0.2,
                        "quality": 0.05,
                        "future-effect": {"value": 7}
                    }
                }
            }
        }"#,
    )
    .unwrap();
    let module = report
        .catalog()
        .module(&ModuleId::new("future-module").unwrap())
        .unwrap();

    assert_close(module.speed_effect().get(), 0.1);
    assert_eq!(
        module.unsupported_effects(),
        &["future-effect", "pollution", "quality"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert!(!module.is_selectable());

    for path in [
        "/module/future-module/effect/future-effect",
        "/module/future-module/effect/pollution",
        "/module/future-module/effect/quality",
    ] {
        assert!(report.diagnostics().iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.prototype_type.as_deref() == Some("module")
                && diagnostic.prototype_id.as_deref() == Some("future-module")
                && diagnostic.path == path
                && diagnostic.disposition == PrototypeDisposition::PartiallyRetained
        }));
    }
}

#[test]
fn reports_malformed_module_fuel_and_belt_fields_with_precise_context() {
    let diagnostics = invalid_data(
        r#"{
            "item": {
                "bad-energy": {
                    "type": "item",
                    "name": "bad-energy",
                    "fuel_category": "chemical",
                    "fuel_value": "not-energy"
                },
                "missing-category": {
                    "type": "item",
                    "name": "missing-category",
                    "fuel_value": "1MJ"
                },
                "missing-burnt-result": {
                    "type": "item",
                    "name": "missing-burnt-result",
                    "fuel_category": "chemical",
                    "fuel_value": "1MJ",
                    "burnt_result": "missing"
                }
            },
            "module": {
                "bad-module": {
                    "type": "module",
                    "name": "bad-module",
                    "category": "",
                    "effect": {
                        "speed": "fast",
                        "productivity": null,
                        "consumption": []
                    }
                }
            },
            "transport-belt": {
                "zero-belt": {
                    "type": "transport-belt",
                    "name": "zero-belt",
                    "speed": 0
                },
                "negative-belt": {
                    "type": "transport-belt",
                    "name": "negative-belt",
                    "speed": -1
                },
                "string-belt": {
                    "type": "transport-belt",
                    "name": "string-belt",
                    "speed": "fast"
                }
            }
        }"#,
    );

    for path in [
        "/item/bad-energy/fuel_value",
        "/item/missing-category/fuel_category",
        "/item/missing-burnt-result/burnt_result",
        "/module/bad-module/category",
        "/module/bad-module/effect/speed",
        "/module/bad-module/effect/productivity",
        "/module/bad-module/effect/consumption",
        "/transport-belt/zero-belt/speed",
        "/transport-belt/negative-belt/speed",
        "/transport-belt/string-belt/speed",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}: {diagnostics:#?}"
        );
    }
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.disposition == PrototypeDisposition::Rejected
    }));
}

#[test]
fn reports_malformed_module_and_belt_collections() {
    let diagnostics = invalid_data(
        r#"{
            "module": [],
            "transport-belt": []
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.prototype_type.as_deref() == Some("module") && diagnostic.path == "/module"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.prototype_type.as_deref() == Some("transport-belt")
            && diagnostic.path == "/transport-belt"
    }));
}

#[test]
fn imports_assemblers_furnaces_and_machine_defaults() {
    let report = import(include_str!("fixtures/crafting-machines-data-raw.json")).unwrap();
    let catalog = report.catalog();

    assert_eq!(catalog.machines().len(), 5);

    let defaults = catalog
        .machine(&MachineId::new("assembler-defaults").unwrap())
        .unwrap();
    assert_close(defaults.crafting_speed().get(), 1.25);
    assert_eq!(defaults.module_slots(), 0);
    assert!(defaults.allowed_effects().is_empty());
    assert_eq!(defaults.allowed_module_categories(), None);
    assert_close(defaults.energy_usage().get(), 90_000.0);
    assert!(defaults.supports_category(&RecipeCategory::new("modded-crafting").unwrap()));
    assert_close(defaults.crafts_per_second(Positive::new(0.5).unwrap()), 2.5);
    assert!(matches!(
        defaults.energy_source(),
        MachineEnergySource::Electric { drain }
            if (drain.get() - 3_000.0).abs() < f64::EPSILON
    ));

    let explicit = catalog
        .machine(&MachineId::new("assembler-explicit").unwrap())
        .unwrap();
    assert_eq!(explicit.module_slots(), 4);
    assert_eq!(
        explicit.allowed_effects(),
        &[
            ModuleEffect::Speed,
            ModuleEffect::Productivity,
            ModuleEffect::Consumption
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        explicit.allowed_module_categories(),
        Some(
            &[
                ModuleCategory::new("productivity").unwrap(),
                ModuleCategory::new("speed").unwrap()
            ]
            .into_iter()
            .collect()
        )
    );
    assert_close(explicit.energy_usage().get(), 2_000_000.0);
    assert!(matches!(
        explicit.energy_source(),
        MachineEnergySource::Electric { drain }
            if (drain.get() - 1_000.0).abs() < f64::EPSILON
    ));

    let stone_furnace = catalog
        .machine(&MachineId::new("stone-furnace").unwrap())
        .unwrap();
    assert!(matches!(
        stone_furnace.energy_source(),
        MachineEnergySource::Burner {
            fuel_categories,
            effectivity
        } if fuel_categories == &[FuelCategory::new("chemical").unwrap()].into_iter().collect()
            && (effectivity.get() - 1.0).abs() < f64::EPSILON
    ));

    assert_eq!(
        catalog.machines_for_category(&RecipeCategory::new("crafting").unwrap()),
        &[
            MachineId::new("assembler-defaults").unwrap(),
            MachineId::new("assembler-explicit").unwrap(),
            MachineId::new("heat-assembler").unwrap()
        ]
    );
}

#[test]
fn retains_unsupported_energy_sources_with_warnings() {
    let report = import(include_str!("fixtures/crafting-machines-data-raw.json")).unwrap();
    let machine = report
        .catalog()
        .machine(&MachineId::new("heat-assembler").unwrap())
        .unwrap();

    assert_close(machine.energy_usage().get(), 180_000_000.0);
    assert_eq!(
        machine.energy_source(),
        &MachineEnergySource::Unsupported(UnsupportedEnergySource::Heat)
    );
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.prototype_type.as_deref() == Some("assembling-machine")
            && diagnostic.prototype_id.as_deref() == Some("heat-assembler")
            && diagnostic.path == "/assembling-machine/heat-assembler/energy_source/type"
            && diagnostic.disposition == PrototypeDisposition::PartiallyRetained
    }));
}

#[test]
fn normalizes_machine_power_units_and_single_allowed_effects() {
    let report = import(
        r#"{
            "assembling-machine": {
                "watts": {
                    "type": "assembling-machine",
                    "name": "watts",
                    "crafting_categories": ["crafting"],
                    "crafting_speed": 1,
                    "allowed_effects": "speed",
                    "energy_usage": "90W",
                    "energy_source": {
                        "type": "electric",
                        "usage_priority": "secondary-input",
                        "drain": "2J"
                    }
                },
                "kilowatts": {
                    "type": "assembling-machine",
                    "name": "kilowatts",
                    "crafting_categories": ["crafting"],
                    "crafting_speed": 1,
                    "energy_usage": "1.5kW",
                    "energy_source": {
                        "type": "electric",
                        "usage_priority": "secondary-input"
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let watts = report
        .catalog()
        .machine(&MachineId::new("watts").unwrap())
        .unwrap();
    assert_eq!(
        watts.allowed_effects(),
        &[ModuleEffect::Speed].into_iter().collect()
    );
    assert_close(watts.energy_usage().get(), 90.0);
    assert!(matches!(
        watts.energy_source(),
        MachineEnergySource::Electric { drain }
            if (drain.get() - 120.0).abs() < f64::EPSILON
    ));

    let kilowatts = report
        .catalog()
        .machine(&MachineId::new("kilowatts").unwrap())
        .unwrap();
    assert_close(kilowatts.energy_usage().get(), 1_500.0);
}

#[test]
fn reports_malformed_machine_fields_with_precise_context() {
    let diagnostics = invalid_data(
        r#"{
            "assembling-machine": {
                "bad": {
                    "type": "assembling-machine",
                    "name": "bad",
                    "crafting_categories": ["crafting", 7, ""],
                    "crafting_speed": 0,
                    "module_slots": -1,
                    "allowed_effects": ["speed", 7],
                    "allowed_module_categories": ["speed", ""],
                    "energy_usage": "not-power",
                    "energy_source": {
                        "type": "burner",
                        "effectivity": 0,
                        "fuel_categories": ["chemical", 7]
                    }
                }
            },
            "furnace": []
        }"#,
    );

    for path in [
        "/assembling-machine/bad/crafting_categories/1",
        "/assembling-machine/bad/crafting_categories/2",
        "/assembling-machine/bad/crafting_speed",
        "/assembling-machine/bad/module_slots",
        "/assembling-machine/bad/allowed_effects/1",
        "/assembling-machine/bad/allowed_module_categories/1",
        "/assembling-machine/bad/energy_usage",
        "/assembling-machine/bad/energy_source/effectivity",
        "/assembling-machine/bad/energy_source/fuel_categories/1",
        "/furnace",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}: {diagnostics:#?}"
        );
    }
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.disposition == PrototypeDisposition::Rejected
    }));
}

#[test]
fn warns_about_unknown_machine_effects_and_energy_sources() {
    let report = import(
        r#"{
            "assembling-machine": {
                "future-machine": {
                    "type": "assembling-machine",
                    "name": "future-machine",
                    "crafting_categories": ["crafting"],
                    "crafting_speed": 1,
                    "allowed_effects": ["speed", "future-effect"],
                    "energy_usage": "1kW",
                    "energy_source": {"type": "future-power"}
                }
            }
        }"#,
    )
    .unwrap();

    let machine = report
        .catalog()
        .machine(&MachineId::new("future-machine").unwrap())
        .unwrap();
    assert_eq!(
        machine.allowed_effects(),
        &[ModuleEffect::Speed].into_iter().collect()
    );
    assert_eq!(
        machine.energy_source(),
        &MachineEnergySource::Unsupported(UnsupportedEnergySource::Unknown("future-power".into()))
    );
    assert_eq!(
        report
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count(),
        2
    );
}

#[test]
fn imports_minimal_item_and_fluid_recipes() {
    let report = import(include_str!("fixtures/minimal-data-raw.json")).unwrap();
    let catalog = report.catalog();

    assert!(report.diagnostics().is_empty());
    assert_eq!(catalog.commodities().len(), 7);
    assert!(catalog.commodity(&item("shared-name")).is_some());
    assert!(catalog.commodity(&fluid("shared-name")).is_some());

    let plate = catalog
        .recipe(&RecipeId::new("iron-plate").unwrap())
        .unwrap();
    assert_eq!(plate.category(), &RecipeCategory::new("smelting").unwrap());
    assert_close(plate.duration().get(), 3.2);
    assert!(!plate.visible());
    assert_eq!(plate.main_product(), Some(&item("iron-plate")));
    assert_eq!(plate.ingredients()[0].commodity(), &item("iron-ore"));
    assert_close(plate.ingredients()[0].amount().get(), 1.0);
    assert_eq!(plate.products()[0].commodity(), &item("iron-plate"));
    assert_close(plate.products()[0].amount().get(), 1.0);

    let steam = catalog.recipe(&RecipeId::new("steam").unwrap()).unwrap();
    assert_eq!(steam.ingredients()[0].commodity(), &fluid("water"));
    assert_eq!(steam.products()[0].commodity(), &fluid("steam"));
    assert_eq!(steam.main_product(), Some(&fluid("steam")));
}

#[test]
fn applies_factorio_recipe_defaults() {
    let report = import(include_str!("fixtures/minimal-data-raw.json")).unwrap();
    let recipe = report
        .catalog()
        .recipe(&RecipeId::new("free-item").unwrap())
        .unwrap();

    assert_eq!(recipe.category(), &RecipeCategory::new("crafting").unwrap());
    assert_close(recipe.duration().get(), 0.5);
    assert!(recipe.visible());
    assert!(recipe.ingredients().is_empty());
    assert_eq!(recipe.main_product(), Some(&item("free-item")));
}

#[test]
fn leaves_main_product_unset_for_multiple_products_or_an_explicit_empty_value() {
    let report = import(
        r#"{
            "item": {
                "a": {"type": "item", "name": "a"},
                "b": {"type": "item", "name": "b"}
            },
            "recipe": {
                "multiple": {
                    "type": "recipe",
                    "name": "multiple",
                    "results": [
                        {"type": "item", "name": "a", "amount": 1},
                        {"type": "item", "name": "b", "amount": 1}
                    ]
                },
                "explicit-empty": {
                    "type": "recipe",
                    "name": "explicit-empty",
                    "results": [{"type": "item", "name": "a", "amount": 1}],
                    "main_product": ""
                }
            }
        }"#,
    )
    .unwrap();

    for recipe_id in ["multiple", "explicit-empty"] {
        assert_eq!(
            report
                .catalog()
                .recipe(&RecipeId::new(recipe_id).unwrap())
                .unwrap()
                .main_product(),
            None
        );
    }
}

#[test]
fn ignores_unrelated_top_level_collections() {
    let report = import(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "result": {
                    "type": "recipe",
                    "name": "result",
                    "results": [{"type": "item", "name": "result", "amount": 1}]
                }
            },
            "noise": {"not": ["a", "prototype", "map"]}
        }"#,
    )
    .unwrap();

    assert_eq!(report.catalog().recipes().len(), 1);
    assert!(report.diagnostics().is_empty());
}

#[test]
fn imports_item_like_prototype_collections_as_item_commodities() {
    let report = import(
        r#"{
            "ammo": {"firearm-magazine": {"type": "ammo", "name": "firearm-magazine"}},
            "armor": {"light-armor": {"type": "armor", "name": "light-armor"}},
            "blueprint": {"blueprint": {"type": "blueprint", "name": "blueprint"}},
            "blueprint-book": {"blueprint-book": {"type": "blueprint-book", "name": "blueprint-book"}},
            "capsule": {"grenade": {"type": "capsule", "name": "grenade"}},
            "copy-paste-tool": {"cut-paste-tool": {"type": "copy-paste-tool", "name": "cut-paste-tool"}},
            "deconstruction-item": {"deconstruction-planner": {"type": "deconstruction-item", "name": "deconstruction-planner"}},
            "gun": {"pistol": {"type": "gun", "name": "pistol"}},
            "item": {"iron-plate": {"type": "item", "name": "iron-plate"}},
            "item-with-entity-data": {"locomotive": {"type": "item-with-entity-data", "name": "locomotive"}},
            "rail-planner": {"rail": {"type": "rail-planner", "name": "rail"}},
            "repair-tool": {"repair-pack": {"type": "repair-tool", "name": "repair-pack"}},
            "selection-tool": {"selection-tool": {"type": "selection-tool", "name": "selection-tool"}},
            "spidertron-remote": {"spidertron-remote": {"type": "spidertron-remote", "name": "spidertron-remote"}},
            "tool": {"automation-science-pack": {"type": "tool", "name": "automation-science-pack"}},
            "upgrade-item": {"upgrade-planner": {"type": "upgrade-item", "name": "upgrade-planner"}},
            "recipe": {
                "mixed-products": {
                    "type": "recipe",
                    "name": "mixed-products",
                    "ingredients": [{"type": "item", "name": "iron-plate", "amount": 1}],
                    "results": [
                        {"type": "item", "name": "firearm-magazine", "amount": 1},
                        {"type": "item", "name": "light-armor", "amount": 1},
                        {"type": "item", "name": "blueprint", "amount": 1},
                        {"type": "item", "name": "blueprint-book", "amount": 1},
                        {"type": "item", "name": "grenade", "amount": 1},
                        {"type": "item", "name": "cut-paste-tool", "amount": 1},
                        {"type": "item", "name": "deconstruction-planner", "amount": 1},
                        {"type": "item", "name": "pistol", "amount": 1},
                        {"type": "item", "name": "locomotive", "amount": 1},
                        {"type": "item", "name": "rail", "amount": 1},
                        {"type": "item", "name": "repair-pack", "amount": 1},
                        {"type": "item", "name": "selection-tool", "amount": 1},
                        {"type": "item", "name": "spidertron-remote", "amount": 1},
                        {"type": "item", "name": "automation-science-pack", "amount": 1},
                        {"type": "item", "name": "upgrade-planner", "amount": 1}
                    ]
                }
            }
        }"#,
    )
    .unwrap();

    for name in [
        "firearm-magazine",
        "light-armor",
        "blueprint",
        "blueprint-book",
        "grenade",
        "cut-paste-tool",
        "deconstruction-planner",
        "pistol",
        "locomotive",
        "rail",
        "repair-pack",
        "selection-tool",
        "spidertron-remote",
        "automation-science-pack",
        "upgrade-planner",
    ] {
        assert!(
            report.catalog().commodity(&item(name)).is_some(),
            "missing item-like commodity {name}"
        );
    }
    assert!(report.diagnostics().is_empty());
}

#[test]
fn ignores_parameter_recipes_without_results() {
    let report = import(
        r#"{
            "item": {"iron-plate": {"type": "item", "name": "iron-plate"}},
            "recipe": {
                "parameter-0": {
                    "type": "recipe",
                    "name": "parameter-0",
                    "category": "parameters",
                    "parameter": true
                },
                "iron-plate": {
                    "type": "recipe",
                    "name": "iron-plate",
                    "results": [{"type": "item", "name": "iron-plate", "amount": 1}]
                }
            }
        }"#,
    )
    .unwrap();

    assert!(
        report
            .catalog()
            .recipe(&RecipeId::new("parameter-0").unwrap())
            .is_none()
    );
    assert!(
        report
            .catalog()
            .recipe(&RecipeId::new("iron-plate").unwrap())
            .is_some()
    );
    assert!(report.diagnostics().is_empty());
}

#[test]
fn ignores_hidden_empty_placeholder_recipes() {
    let report = import(
        r#"{
            "item": {"iron-plate": {"type": "item", "name": "iron-plate"}},
            "recipe": {
                "recipe-unknown": {
                    "type": "recipe",
                    "name": "recipe-unknown",
                    "hidden": true,
                    "ingredients": {},
                    "results": {}
                },
                "iron-plate": {
                    "type": "recipe",
                    "name": "iron-plate",
                    "results": [{"type": "item", "name": "iron-plate", "amount": 1}]
                }
            }
        }"#,
    )
    .unwrap();

    assert!(
        report
            .catalog()
            .recipe(&RecipeId::new("recipe-unknown").unwrap())
            .is_none()
    );
    assert!(
        report
            .catalog()
            .recipe(&RecipeId::new("iron-plate").unwrap())
            .is_some()
    );
    assert!(report.diagnostics().is_empty());
}

#[test]
fn rejects_non_parameter_recipes_without_results() {
    let diagnostics = invalid_data(
        r#"{
            "recipe": {
                "broken": {
                    "type": "recipe",
                    "name": "broken"
                }
            }
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/broken/results"
            && diagnostic.message == "missing required results"
    }));
}

#[test]
fn rejects_non_placeholder_object_recipe_fields() {
    let diagnostics = invalid_data(
        r#"{
            "recipe": {
                "broken": {
                    "type": "recipe",
                    "name": "broken",
                    "ingredients": {},
                    "results": {}
                }
            }
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/broken/ingredients"
            && diagnostic.message == "ingredients must be an array"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/broken/results"
            && diagnostic.message == "results must be an array"
    }));
}

#[test]
fn reports_malformed_supported_fields_with_precise_context() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "bad": {
                    "type": "recipe",
                    "name": "bad",
                    "category": 7,
                    "energy_required": 0,
                    "hidden": "yes",
                    "ingredients": [
                        {"type": "item", "name": "result", "amount": -1}
                    ],
                    "results": [
                        {"type": "virtual", "name": "result", "amount": 1}
                    ]
                }
            }
        }"#,
    );

    for path in [
        "/recipe/bad/category",
        "/recipe/bad/energy_required",
        "/recipe/bad/hidden",
        "/recipe/bad/ingredients/0/amount",
        "/recipe/bad/results/0/type",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}: {diagnostics:#?}"
        );
    }
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.prototype_type.as_deref() == Some("recipe")
            && diagnostic.prototype_id.as_deref() == Some("bad")
            && diagnostic.disposition == PrototypeDisposition::Rejected
            && !diagnostic.message.is_empty()
    }));
}

#[test]
fn rejects_invalid_fixed_amount_shapes() {
    let diagnostics = invalid_data(
        r#"{
            "item": {
                "input": {"type": "item", "name": "input"},
                "result": {"type": "item", "name": "result"}
            },
            "recipe": {
                "bad-amounts": {
                    "type": "recipe",
                    "name": "bad-amounts",
                    "ingredients": [
                        {"type": "item", "name": "input"},
                        {"type": "item", "name": "input", "amount": "one"},
                        {"type": "item", "name": "input", "amount": 0}
                    ],
                    "results": [
                        {"type": "item", "name": "result", "amount": -1}
                    ]
                }
            }
        }"#,
    );

    for path in [
        "/recipe/bad-amounts/ingredients/0/amount",
        "/recipe/bad-amounts/ingredients/1/amount",
        "/recipe/bad-amounts/ingredients/2/amount",
        "/recipe/bad-amounts/results/0/amount",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}: {diagnostics:#?}"
        );
    }
}

#[test]
fn normalizes_expected_product_amounts_and_aggregates_duplicates() {
    let report = import(include_str!("fixtures/expected-products-data-raw.json")).unwrap();
    let recipe = report
        .catalog()
        .recipe(&RecipeId::new("expected-products").unwrap())
        .unwrap();

    assert!(report.diagnostics().is_empty());
    let products = recipe.products();
    assert_eq!(products.len(), 6);
    assert_eq!(products[0].commodity(), &item("fixed"));
    assert_close(products[0].amount().get(), 4.0);
    assert_eq!(products[1].commodity(), &item("ranged"));
    assert_close(products[1].amount().get(), 4.0);
    assert_eq!(products[2].commodity(), &item("probabilistic"));
    assert_close(products[2].amount().get(), 2.5);
    assert_eq!(products[3].commodity(), &item("combined"));
    assert_close(products[3].amount().get(), 3.0);
    assert_eq!(products[4].commodity(), &item("duplicate"));
    assert_close(products[4].amount().get(), 2.5);
    assert_eq!(products[5].commodity(), &fluid("duplicate"));
    assert_close(products[5].amount().get(), 5.0);
    assert_eq!(recipe.main_product(), None);
}

#[test]
fn infers_and_validates_main_product_after_duplicate_aggregation() {
    let report = import(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "inferred": {
                    "type": "recipe",
                    "name": "inferred",
                    "results": [
                        {"type": "item", "name": "result", "amount": 1},
                        {"type": "item", "name": "result", "amount": 2}
                    ]
                },
                "explicit": {
                    "type": "recipe",
                    "name": "explicit",
                    "results": [
                        {"type": "item", "name": "result", "amount": 1},
                        {"type": "item", "name": "result", "amount": 2}
                    ],
                    "main_product": "result"
                }
            }
        }"#,
    )
    .unwrap();

    for recipe_id in ["inferred", "explicit"] {
        let recipe = report
            .catalog()
            .recipe(&RecipeId::new(recipe_id).unwrap())
            .unwrap();
        assert_eq!(recipe.products().len(), 1);
        assert_close(recipe.products()[0].amount().get(), 3.0);
        assert_eq!(recipe.main_product(), Some(&item("result")));
    }
}

#[test]
fn clamps_reversed_product_ranges_and_prefers_fixed_amounts() {
    let report = import(
        r#"{
            "item": {
                "clamped": {"type": "item", "name": "clamped"},
                "fixed": {"type": "item", "name": "fixed"}
            },
            "recipe": {
                "range-rules": {
                    "type": "recipe",
                    "name": "range-rules",
                    "results": [
                        {
                            "type": "item",
                            "name": "clamped",
                            "amount_min": 5,
                            "amount_max": 2
                        },
                        {
                            "type": "item",
                            "name": "fixed",
                            "amount": 4,
                            "amount_min": "ignored",
                            "amount_max": "ignored"
                        }
                    ]
                }
            }
        }"#,
    )
    .unwrap();

    let recipe = report
        .catalog()
        .recipe(&RecipeId::new("range-rules").unwrap())
        .unwrap();
    assert_close(recipe.products()[0].amount().get(), 5.0);
    assert_close(recipe.products()[1].amount().get(), 4.0);
}

#[test]
fn allows_zero_product_rows_when_the_commodity_aggregate_is_positive() {
    let report = import(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "some-output": {
                    "type": "recipe",
                    "name": "some-output",
                    "results": [
                        {
                            "type": "item",
                            "name": "result",
                            "amount": 10,
                            "probability": 0
                        },
                        {"type": "item", "name": "result", "amount": 2}
                    ]
                }
            }
        }"#,
    )
    .unwrap();

    let product = &report
        .catalog()
        .recipe(&RecipeId::new("some-output").unwrap())
        .unwrap()
        .products()[0];
    assert_close(product.amount().get(), 2.0);
}

#[test]
fn reports_invalid_product_amount_ranges_and_probabilities() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "invalid-products": {
                    "type": "recipe",
                    "name": "invalid-products",
                    "results": [
                        {"type": "item", "name": "result", "amount_min": 1},
                        {"type": "item", "name": "result", "amount_max": 2},
                        {
                            "type": "item",
                            "name": "result",
                            "amount_min": -1,
                            "amount_max": 2
                        },
                        {"type": "item", "name": "result", "amount": 1, "probability": "often"},
                        {"type": "item", "name": "result", "amount": 1, "probability": -0.1},
                        {"type": "item", "name": "result", "amount": 1, "probability": 1.1}
                    ]
                }
            }
        }"#,
    );

    for path in [
        "/recipe/invalid-products/results/0/amount_max",
        "/recipe/invalid-products/results/1/amount_min",
        "/recipe/invalid-products/results/2/amount_min",
        "/recipe/invalid-products/results/3/probability",
        "/recipe/invalid-products/results/4/probability",
        "/recipe/invalid-products/results/5/probability",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}: {diagnostics:#?}"
        );
    }
}

#[test]
fn rejects_zero_aggregated_expected_product_output() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "no-output": {
                    "type": "recipe",
                    "name": "no-output",
                    "results": [
                        {
                            "type": "item",
                            "name": "result",
                            "amount": 1,
                            "probability": 0
                        },
                        {
                            "type": "item",
                            "name": "result",
                            "amount_min": 0,
                            "amount_max": 0
                        }
                    ]
                }
            }
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/no-output/results/0"
            && diagnostic.message.contains("expected output")
    }));
}

#[test]
fn reports_malformed_collections_and_prototypes() {
    let diagnostics = invalid_data(
        r#"{
            "item": [],
            "fluid": {"water": "not an object"},
            "recipe": {}
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/item"
            && diagnostic.prototype_type.as_deref() == Some("item")
            && diagnostic.prototype_id.is_none()
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/fluid/water"
            && diagnostic.prototype_type.as_deref() == Some("fluid")
            && diagnostic.prototype_id.as_deref() == Some("water")
    }));
}

#[test]
fn reports_broken_references_at_the_source_field() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"result": {"type": "item", "name": "result"}},
            "recipe": {
                "broken": {
                    "type": "recipe",
                    "name": "broken",
                    "ingredients": [
                        {"type": "fluid", "name": "missing-water", "amount": 10}
                    ],
                    "results": [
                        {"type": "item", "name": "result", "amount": 1}
                    ],
                    "main_product": "missing-result"
                }
            }
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/broken/ingredients/0/name"
            && diagnostic.message.contains("missing-water")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/broken/main_product"
            && diagnostic.message.contains("missing-result")
    }));
}

#[test]
fn rejects_mismatched_prototype_identity() {
    let diagnostics = invalid_data(
        r#"{
            "item": {
                "expected-name": {"type": "fluid", "name": "different-name"}
            },
            "recipe": {}
        }"#,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/item/expected-name/type" && diagnostic.message.contains("item")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/item/expected-name/name"
            && diagnostic.message.contains("expected-name")
    }));
}

#[test]
fn reports_json_syntax_locations_separately() {
    let error = import("{\n  \"item\": {\n").unwrap_err();

    match error {
        ImportError::Json {
            line,
            column,
            message,
        } => {
            assert!(line >= 2);
            assert!(column >= 1);
            assert!(!message.is_empty());
        }
        other @ ImportError::InvalidData { .. } => {
            panic!("expected JSON syntax error, got {other:?}")
        }
    }
}

#[test]
fn imports_recipe_module_policy_and_productivity_exclusions() {
    let report = import(
        r#"{
            "item": {
                "ore": {"type": "item", "name": "ore"},
                "plate": {"type": "item", "name": "plate"},
                "slag": {"type": "item", "name": "slag"}
            },
            "recipe": {
                "smelt": {
                    "type": "recipe",
                    "name": "smelt",
                    "allow_speed": false,
                    "allow_productivity": true,
                    "allow_consumption": true,
                    "allowed_module_categories": ["productivity"],
                    "maximum_productivity": 0.25,
                    "ingredients": [
                        {"type": "item", "name": "ore", "amount": 1}
                    ],
                    "results": [
                        {
                            "type": "item",
                            "name": "plate",
                            "amount_min": 2,
                            "amount_max": 6,
                            "probability": 0.5,
                            "ignored_by_productivity": 1
                        },
                        {
                            "type": "item",
                            "name": "slag",
                            "amount": 2,
                            "ignored_by_stats": 2
                        }
                    ]
                }
            }
        }"#,
    )
    .unwrap();
    let recipe = report
        .catalog()
        .recipe(&RecipeId::new("smelt").unwrap())
        .unwrap();

    assert_eq!(
        recipe.allowed_effects(),
        &[ModuleEffect::Productivity, ModuleEffect::Consumption]
            .into_iter()
            .collect()
    );
    assert_eq!(
        recipe.allowed_module_categories(),
        Some(
            &[ModuleCategory::new("productivity").unwrap()]
                .into_iter()
                .collect()
        )
    );
    assert_close(recipe.maximum_productivity().get(), 0.25);
    let plate = recipe
        .products()
        .iter()
        .find(|product| product.commodity() == &item("plate"))
        .unwrap();
    assert_close(plate.amount().get(), 2.0);
    assert_close(plate.productivity_amount().get(), 1.5);
    let slag = recipe
        .products()
        .iter()
        .find(|product| product.commodity() == &item("slag"))
        .unwrap();
    assert_close(slag.productivity_amount().get(), 0.0);
}

#[test]
fn applies_factorio_recipe_module_defaults() {
    let report = import(
        r#"{
            "item": {"plate": {"type": "item", "name": "plate"}},
            "recipe": {
                "plate": {
                    "type": "recipe",
                    "name": "plate",
                    "results": [{"type": "item", "name": "plate", "amount": 1}]
                }
            }
        }"#,
    )
    .unwrap();
    let recipe = report
        .catalog()
        .recipe(&RecipeId::new("plate").unwrap())
        .unwrap();

    assert!(recipe.allowed_effects().contains(&ModuleEffect::Speed));
    assert!(
        recipe
            .allowed_effects()
            .contains(&ModuleEffect::Consumption)
    );
    assert!(
        !recipe
            .allowed_effects()
            .contains(&ModuleEffect::Productivity)
    );
    assert_eq!(recipe.allowed_module_categories(), None);
    assert_close(recipe.maximum_productivity().get(), 3.0);
    assert_close(recipe.products()[0].productivity_amount().get(), 1.0);
}

#[test]
fn reports_malformed_recipe_module_policy_fields() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"plate": {"type": "item", "name": "plate"}},
            "recipe": {
                "plate": {
                    "type": "recipe",
                    "name": "plate",
                    "allow_speed": "yes",
                    "allow_productivity": 1,
                    "allow_consumption": [],
                    "allowed_module_categories": ["speed", ""],
                    "maximum_productivity": -1,
                    "results": [{"type": "item", "name": "plate", "amount": 1}]
                }
            }
        }"#,
    );

    for path in [
        "/recipe/plate/allow_speed",
        "/recipe/plate/allow_productivity",
        "/recipe/plate/allow_consumption",
        "/recipe/plate/allowed_module_categories/1",
        "/recipe/plate/maximum_productivity",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.path == path),
            "missing diagnostic for {path}"
        );
    }
}

#[test]
fn reports_malformed_product_productivity_fields() {
    let diagnostics = invalid_data(
        r#"{
            "item": {"plate": {"type": "item", "name": "plate"}},
            "recipe": {
                "plate": {
                    "type": "recipe",
                    "name": "plate",
                    "results": [{
                        "type": "item",
                        "name": "plate",
                        "amount": 1,
                        "ignored_by_stats": -1,
                        "ignored_by_productivity": "all"
                    }]
                }
            }
        }"#,
    );

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.path == "/recipe/plate/results/0/ignored_by_stats" })
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "/recipe/plate/results/0/ignored_by_productivity"
    }));
}
