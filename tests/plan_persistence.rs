use std::fs;

use factorio_planner_tui::catalog::{
    CommodityId, FluidId, ItemId, ProductionSource, RecipeId, ResourceSourceId,
};
use factorio_planner_tui::persistence::{
    MissingPlanReference, PlanDocument, PlanFileError, PlanFileStore, PlanName,
    PlanOpenBlockReason, PlanOpenResult, ProfileImportRequest, ProfileName, ProfileStore,
};
use factorio_planner_tui::planner::{FactoryPlan, RateUnit, Target};
use serde_json::Value;
use tempfile::TempDir;

fn profile_name(name: &str) -> ProfileName {
    ProfileName::new(name).expect("test profile name should be valid")
}

fn plan_name(name: &str) -> PlanName {
    PlanName::new(name).expect("test plan name should be valid")
}

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
}

fn fluid(name: &str) -> CommodityId {
    CommodityId::Fluid(FluidId::new(name).expect("test fluid ID should be valid"))
}

fn recipe_id(name: &str) -> RecipeId {
    RecipeId::new(name).expect("test recipe ID should be valid")
}

fn resource_id(name: &str) -> ResourceSourceId {
    ResourceSourceId::new(name).expect("test resource source ID should be valid")
}

fn target(commodity: CommodityId, rate_per_second: f64) -> Target {
    Target::new(commodity, rate_per_second).expect("test target should be valid")
}

fn write_data(directory: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn full_data() -> &'static str {
    r#"{
        "item": {
            "iron-ore": {"type": "item", "name": "iron-ore"},
            "iron-plate": {"type": "item", "name": "iron-plate"},
            "coal": {
                "type": "item",
                "name": "coal",
                "fuel_category": "chemical",
                "fuel_value": "8MJ"
            }
        },
        "fluid": {
            "water": {"type": "fluid", "name": "water"},
            "steam": {"type": "fluid", "name": "steam"}
        },
        "recipe": {
            "iron-plate": {
                "type": "recipe",
                "name": "iron-plate",
                "category": "crafting",
                "energy_required": 1,
                "ingredients": [{"type": "item", "name": "iron-ore", "amount": 1}],
                "results": [{"type": "item", "name": "iron-plate", "amount": 1}]
            },
            "steam": {
                "type": "recipe",
                "name": "steam",
                "category": "chemistry",
                "energy_required": 1,
                "ingredients": [{"type": "fluid", "name": "water", "amount": 10}],
                "results": [{"type": "fluid", "name": "steam", "amount": 10}]
            }
        },
        "assembling-machine": {
            "assembler": {
                "type": "assembling-machine",
                "name": "assembler",
                "crafting_categories": ["crafting", "chemistry"],
                "crafting_speed": 1,
                "module_slots": 2,
                "allowed_effects": ["speed", "consumption"],
                "allowed_module_categories": ["speed"],
                "energy_usage": "90kW",
                "energy_source": {"type": "electric", "usage_priority": "secondary-input"}
            }
        },
        "module": {
            "speed-module": {
                "type": "module",
                "name": "speed-module",
                "category": "speed",
                "effect": {"speed": 0.2, "consumption": 0.5}
            }
        },
        "transport-belt": {
            "transport-belt": {
                "type": "transport-belt",
                "name": "transport-belt",
                "speed": 0.03125
            }
        }
    }"#
}

fn minimal_data(result_name: &str) -> String {
    format!(
        r#"{{
            "item": {{
                "iron-ore": {{"type": "item", "name": "iron-ore"}},
                "{result_name}": {{"type": "item", "name": "{result_name}"}}
            }},
            "recipe": {{
                "{result_name}": {{
                    "type": "recipe",
                    "name": "{result_name}",
                    "ingredients": [{{"type": "item", "name": "iron-ore", "amount": 1}}],
                    "results": [{{"type": "item", "name": "{result_name}", "amount": 1}}]
                }}
            }}
        }}"#
    )
}

fn create_profile(
    root: &TempDir,
    sources: &TempDir,
    name: &str,
    data_name: &str,
    data: &str,
) -> factorio_planner_tui::persistence::DatasetProfile {
    let store = ProfileStore::new(root.path());
    let data_path = write_data(sources, data_name, data);
    store
        .create(&ProfileImportRequest::new(profile_name(name), &data_path))
        .unwrap()
}

fn sample_plan() -> FactoryPlan {
    let mut plan = FactoryPlan::new(target(item("iron-plate"), 2.5))
        .with_external_inputs([item("iron-ore"), fluid("water")])
        .with_display_rate_unit(RateUnit::Minute)
        .with_selected_belt(factorio_planner_tui::catalog::BeltId::new("transport-belt").unwrap());
    plan.add_target(target(fluid("steam"), 1.25));
    plan.set_recipe_choice(item("iron-plate"), recipe_id("iron-plate"));
    plan.set_machine_choice(
        factorio_planner_tui::catalog::RecipeId::new("iron-plate").unwrap(),
        factorio_planner_tui::catalog::MachineId::new("assembler").unwrap(),
    );
    plan.set_modules(
        item("iron-plate"),
        [
            factorio_planner_tui::catalog::ModuleId::new("speed-module").unwrap(),
            factorio_planner_tui::catalog::ModuleId::new("speed-module").unwrap(),
        ],
    );
    plan.set_fuel_choice(
        item("iron-plate"),
        factorio_planner_tui::catalog::FuelId::new("coal").unwrap(),
    );
    plan
}

fn sample_document(profile: &factorio_planner_tui::persistence::DatasetProfile) -> PlanDocument {
    PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        sample_plan(),
    )
}

#[test]
fn saves_loads_and_opens_a_versioned_factory_plan() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);

    assert!(document.is_dirty());
    file_store.save(&path, &mut document).unwrap();
    assert!(!document.is_dirty());

    let loaded = file_store.load(&path).unwrap();
    assert!(!loaded.is_dirty());
    assert_eq!(loaded.name(), &plan_name("Starter Base"));
    assert_eq!(loaded.dataset_profile(), &profile_name("main"));
    assert_eq!(loaded.dataset_fingerprint(), profile.fingerprint());
    assert_eq!(loaded.plan(), document.plan());

    let opened = file_store
        .open(&path, &ProfileStore::new(root.path()))
        .unwrap();
    let PlanOpenResult::Ready(opened) = opened else {
        panic!("expected exact dataset binding to open ready");
    };
    assert_eq!(opened.plan(), document.plan());
    assert!(!opened.is_dirty());
}

#[test]
fn migrates_legacy_recipe_choices_into_source_choices() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    file_store.save(&path, &mut document).unwrap();

    let mut json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["schema_version"] = Value::from(1);
    json["plan"]
        .as_object_mut()
        .unwrap()
        .remove("source_choices");
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    let loaded = file_store.load(&path).unwrap();
    assert_eq!(
        loaded.plan().source_choice(&item("iron-plate")),
        Some(&ProductionSource::Recipe(recipe_id("iron-plate")))
    );
    assert_eq!(
        loaded.plan().recipe_choice(&item("iron-plate")),
        Some(&recipe_id("iron-plate"))
    );
}

#[test]
fn rejects_conflicting_recipe_and_source_choices() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    file_store.save(&path, &mut document).unwrap();

    let mut json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["plan"]["source_choices"] = Value::from(vec![serde_json::json!({
        "commodity": {"type": "item", "id": "iron-plate"},
        "source": {"kind": "recipe", "recipe": "steam"}
    })]);
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    assert!(matches!(
        file_store.load(&path),
        Err(PlanFileError::InvalidPlan { .. })
    ));
}

#[test]
fn rejects_invalid_plan_files_and_unsupported_versions() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    file_store.save(&path, &mut document).unwrap();

    assert!(matches!(
        file_store.load(&root.path().join("starter.json")),
        Err(PlanFileError::InvalidPlanSuffix { .. })
    ));

    let mut json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["schema_version"] = Value::from(999);
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    assert!(matches!(
        file_store.load(&path),
        Err(PlanFileError::UnsupportedPlanSchema { found: 999, .. })
    ));

    json["schema_version"] = Value::from(1);
    json["plan"]["targets"][0]["rate_per_second"] = Value::from(0.0);
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    assert!(matches!(
        file_store.load(&path),
        Err(PlanFileError::InvalidPlan { .. })
    ));

    json["plan"]["targets"][0]["rate_per_second"] = Value::from(2.5);
    let first_choice = json["plan"]["recipe_choices"][0].clone();
    json["plan"]["recipe_choices"]
        .as_array_mut()
        .unwrap()
        .push(first_choice);
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    assert!(matches!(
        file_store.load(&path),
        Err(PlanFileError::InvalidPlan { .. })
    ));
}

#[test]
fn atomic_save_failure_preserves_existing_plan_and_dirty_state() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    file_store.save(&path, &mut document).unwrap();
    let original = fs::read(&path).unwrap();

    document.edit_plan(|plan| {
        plan.add_target(target(item("iron-plate"), 1.0));
    });
    assert!(document.is_dirty());
    fs::create_dir(path.with_file_name("starter.fptplan.json.tmp")).unwrap();

    assert!(matches!(
        file_store.save(&path, &mut document),
        Err(PlanFileError::Io { .. })
    ));
    assert!(document.is_dirty());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn edits_mark_dirty_only_after_successful_mutation() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    file_store.save(&path, &mut document).unwrap();

    let result =
        document.try_edit_plan(|plan| plan.replace_target(99, target(item("iron-plate"), 1.0)));
    assert!(result.is_err());
    assert!(!document.is_dirty());

    document
        .try_edit_plan(|plan| plan.replace_target(0, target(item("iron-plate"), 3.0)))
        .unwrap();
    assert!(document.is_dirty());
}

#[test]
fn missing_named_profile_falls_back_to_exact_fingerprint_profile() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let main = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&main);
    file_store.save(&path, &mut document).unwrap();

    let profile_store = ProfileStore::new(root.path());
    profile_store.delete(&profile_name("main")).unwrap();
    let alias_data = write_data(&sources, "full-alias.json", full_data());
    let alias = profile_store
        .create(&ProfileImportRequest::new(
            profile_name("alias"),
            alias_data,
        ))
        .unwrap();
    assert_eq!(alias.fingerprint(), main.fingerprint());

    let opened = file_store.open(&path, &profile_store).unwrap();
    let PlanOpenResult::Ready(opened) = opened else {
        panic!("expected exact fingerprint fallback to open ready");
    };
    assert_eq!(opened.dataset_profile(), &profile_name("alias"));
    assert!(opened.is_dirty());
}

#[test]
fn dataset_mismatches_block_opening_until_explicit_rebind() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let main = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let compatible_data = write_data(&sources, "compatible.json", full_data());
    let compatible = profile_store
        .create(&ProfileImportRequest::new(
            profile_name("compatible"),
            compatible_data,
        ))
        .unwrap();
    assert_eq!(compatible.fingerprint(), main.fingerprint());

    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&main);
    file_store.save(&path, &mut document).unwrap();

    let minimal_path = write_data(&sources, "minimal.json", &minimal_data("iron-plate"));
    let incompatible = profile_store
        .replace(&ProfileImportRequest::new(
            profile_name("main"),
            minimal_path,
        ))
        .unwrap();
    assert_ne!(incompatible.fingerprint(), main.fingerprint());

    let opened = file_store.open(&path, &profile_store).unwrap();
    let PlanOpenResult::Blocked(blocked) = opened else {
        panic!("expected dataset mismatch to block opening");
    };
    assert_eq!(
        blocked.reason(),
        &PlanOpenBlockReason::NamedProfileFingerprintMismatch {
            profile: profile_name("main"),
            expected: main.fingerprint().clone(),
            found: incompatible.fingerprint().clone(),
        }
    );

    let missing = file_store
        .rebind(blocked.clone(), &incompatible)
        .expect_err("incompatible profile should be missing persisted references");
    let PlanFileError::MissingReferences { references } = missing else {
        panic!("expected missing references");
    };
    assert!(references.contains(&MissingPlanReference::SelectedBelt(
        factorio_planner_tui::catalog::BeltId::new("transport-belt").unwrap()
    )));
    assert!(
        references.contains(&MissingPlanReference::ModuleChoiceModule {
            commodity: item("iron-plate"),
            module: factorio_planner_tui::catalog::ModuleId::new("speed-module").unwrap(),
        })
    );

    let rebound = file_store.rebind(blocked, &compatible).unwrap();
    assert_eq!(rebound.dataset_profile(), &profile_name("compatible"));
    assert_eq!(rebound.dataset_fingerprint(), compatible.fingerprint());
    assert!(rebound.is_dirty());
}

#[test]
fn reports_missing_source_choice_references_when_rebinding() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile_store = ProfileStore::new(root.path());
    let main = create_profile(&root, &sources, "main", "full.json", full_data());
    let file_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&main);
    document.edit_plan(|plan| {
        plan.set_source_choice(
            item("iron-plate"),
            ProductionSource::Resource(resource_id("iron-ore")),
        );
    });
    file_store.save(&path, &mut document).unwrap();

    let minimal_path = write_data(&sources, "minimal.json", &minimal_data("iron-plate"));
    let incompatible = profile_store
        .replace(&ProfileImportRequest::new(
            profile_name("main"),
            minimal_path,
        ))
        .unwrap();
    let PlanOpenResult::Blocked(blocked) = file_store.open(&path, &profile_store).unwrap() else {
        panic!("expected dataset mismatch to block opening");
    };

    let missing = file_store
        .rebind(blocked, &incompatible)
        .expect_err("incompatible profile should be missing source choice references");
    let PlanFileError::MissingReferences { references } = missing else {
        panic!("expected missing references");
    };
    assert!(
        references.contains(&MissingPlanReference::SourceChoiceSource {
            commodity: item("iron-plate"),
            source: ProductionSource::Resource(resource_id("iron-ore")),
        })
    );
}
