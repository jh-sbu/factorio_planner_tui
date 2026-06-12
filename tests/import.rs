use std::io::Cursor;

use factorio_planner_tui::catalog::{CommodityId, FluidId, ItemId, RecipeCategory, RecipeId};
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
