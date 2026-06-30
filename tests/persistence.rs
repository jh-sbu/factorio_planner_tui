use std::fs;

use factorio_planner_tui::catalog::{CommodityId, ItemId};
use factorio_planner_tui::import::{DiagnosticSeverity, LocalePrototypeKind};
use factorio_planner_tui::persistence::{
    CATALOG_SCHEMA_VERSION, ProfileError, ProfileImportRequest, ProfileName, ProfileStore,
};
use serde_json::Value;
use tempfile::TempDir;

fn profile_name(name: &str) -> ProfileName {
    ProfileName::new(name).expect("test profile name should be valid")
}

fn write_data(directory: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, contents).unwrap();
    path
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

#[test]
fn resolves_profile_paths_under_an_explicit_application_data_root() {
    let directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let fingerprint = factorio_planner_tui::catalog::DatasetFingerprint::new("abc123").unwrap();

    assert_eq!(store.root(), directory.path());
    assert_eq!(store.index_path(), directory.path().join("profiles.json"));
    assert_eq!(store.catalogs_dir(), directory.path().join("catalogs"));
    assert_eq!(
        store.catalog_path(&fingerprint),
        directory.path().join("catalogs/abc123.json")
    );
}

#[test]
fn creates_lists_selects_replaces_deletes_and_reopens_profiles() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let alpha_data = write_data(&source_directory, "alpha.json", &minimal_data("iron-plate"));
    let beta_data = write_data(
        &source_directory,
        "beta.json",
        &minimal_data("copper-plate"),
    );

    let alpha = store
        .create(&ProfileImportRequest::new(
            profile_name("alpha"),
            &alpha_data,
        ))
        .unwrap();
    assert_eq!(
        store.active_profile_name().unwrap(),
        Some(profile_name("alpha"))
    );
    assert_eq!(alpha.name(), &profile_name("alpha"));

    let beta = store
        .create(&ProfileImportRequest::new(profile_name("beta"), &beta_data))
        .unwrap();
    assert_ne!(alpha.fingerprint(), beta.fingerprint());
    assert_eq!(
        store
            .list()
            .unwrap()
            .into_iter()
            .map(|summary| summary.name().as_str().to_owned())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );

    store.select(&profile_name("beta")).unwrap();
    assert_eq!(
        store.active_profile_name().unwrap(),
        Some(profile_name("beta"))
    );

    fs::write(&beta_data, minimal_data("steel-plate")).unwrap();
    let replaced = store
        .replace(&ProfileImportRequest::new(profile_name("beta"), &beta_data))
        .unwrap();
    assert_ne!(beta.fingerprint(), replaced.fingerprint());
    assert_eq!(
        store.active_profile_name().unwrap(),
        Some(profile_name("beta"))
    );

    fs::remove_file(&alpha_data).unwrap();
    fs::remove_file(&beta_data).unwrap();
    let reopened = ProfileStore::new(directory.path())
        .open(&profile_name("beta"))
        .unwrap();
    assert_eq!(reopened.catalog(), replaced.catalog());
    assert!(
        reopened
            .catalog()
            .commodity(&CommodityId::Item(ItemId::new("steel-plate").unwrap()))
            .is_some()
    );

    store.delete(&profile_name("beta")).unwrap();
    assert_eq!(store.active_profile_name().unwrap(), None);
    assert!(matches!(
        store.open(&profile_name("beta")),
        Err(ProfileError::ProfileNotFound { .. })
    ));
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn recipe_support_status_round_trips_and_defaults_for_older_profiles() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let profile = store
        .create(&ProfileImportRequest::new(profile_name("main"), data))
        .unwrap();
    let catalog_path = store.catalog_path(profile.fingerprint());
    let mut catalog_file: Value =
        serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    let recipe = catalog_file["catalog"]["recipes"][0]
        .as_object_mut()
        .unwrap();

    recipe.remove("supported");
    fs::write(
        &catalog_path,
        serde_json::to_vec_pretty(&catalog_file).unwrap(),
    )
    .unwrap();
    let reopened = store.open(&profile_name("main")).unwrap();
    assert!(reopened.catalog().recipes().next().unwrap().supported());

    catalog_file["catalog"]["recipes"][0]["supported"] = Value::Bool(false);
    fs::write(
        &catalog_path,
        serde_json::to_vec_pretty(&catalog_file).unwrap(),
    )
    .unwrap();
    let reopened = store.open(&profile_name("main")).unwrap();
    assert!(!reopened.catalog().recipes().next().unwrap().supported());
}

#[test]
fn rejects_invalid_lifecycle_operations_without_mutating_the_store() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let request = ProfileImportRequest::new(profile_name("main"), &data);

    store.create(&request).unwrap();
    assert!(matches!(
        store.create(&request),
        Err(ProfileError::ProfileAlreadyExists { .. })
    ));
    assert!(matches!(
        store.replace(&ProfileImportRequest::new(profile_name("missing"), &data)),
        Err(ProfileError::ProfileNotFound { .. })
    ));
    assert!(matches!(
        store.select(&profile_name("missing")),
        Err(ProfileError::ProfileNotFound { .. })
    ));
    assert!(matches!(
        store.delete(&profile_name("missing")),
        Err(ProfileError::ProfileNotFound { .. })
    ));

    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        store.active_profile_name().unwrap(),
        Some(profile_name("main"))
    );
}

#[test]
fn fingerprints_source_contents_locale_and_importer_schema_not_paths() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data_contents = minimal_data("iron-plate");
    let first_data = write_data(&source_directory, "first.json", &data_contents);
    let second_data = write_data(&source_directory, "second.json", &data_contents);
    let first_locale = write_data(
        &source_directory,
        "first-item-locale.json",
        r#"{"names":{"iron-plate":"Iron plate"}}"#,
    );
    let second_locale = write_data(
        &source_directory,
        "second-item-locale.json",
        r#"{"names":{"iron-plate":"Iron plate"}}"#,
    );

    let first = store
        .create(
            &ProfileImportRequest::new(profile_name("first"), &first_data)
                .with_locale_path(LocalePrototypeKind::Item, &first_locale),
        )
        .unwrap();
    let second = store
        .create(
            &ProfileImportRequest::new(profile_name("second"), &second_data)
                .with_locale_path(LocalePrototypeKind::Item, &second_locale),
        )
        .unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.metadata().importer_schema_version(),
        factorio_planner_tui::persistence::IMPORTER_SCHEMA_VERSION
    );

    fs::write(
        &second_locale,
        r#"{"names":{"iron-plate":"Refined iron plate"}}"#,
    )
    .unwrap();
    let changed_locale = store
        .replace(
            &ProfileImportRequest::new(profile_name("second"), &second_data)
                .with_locale_path(LocalePrototypeKind::Item, &second_locale),
        )
        .unwrap();
    assert_ne!(first.fingerprint(), changed_locale.fingerprint());

    fs::write(&second_data, minimal_data("steel-plate")).unwrap();
    let changed_data = store
        .replace(
            &ProfileImportRequest::new(profile_name("second"), &second_data)
                .with_locale_path(LocalePrototypeKind::Item, &second_locale),
        )
        .unwrap();
    assert_ne!(changed_locale.fingerprint(), changed_data.fingerprint());
}

#[test]
fn persists_import_metadata_localization_and_warning_summaries() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(
        &source_directory,
        "warnings.json",
        r#"{
            "item": {
                "iron-plate": {"type": "item", "name": "iron-plate"}
            },
            "assembling-machine": {
                "heat-machine": {
                    "type": "assembling-machine",
                    "name": "heat-machine",
                    "crafting_categories": ["crafting"],
                    "crafting_speed": 1,
                    "energy_usage": "90kW",
                    "energy_source": {"type": "heat"}
                }
            }
        }"#,
    );
    let locale = write_data(
        &source_directory,
        "item-locale.json",
        r#"{"names":{"iron-plate":"Iron plate"}}"#,
    );

    let created = store
        .create(
            &ProfileImportRequest::new(profile_name("warnings"), &data)
                .with_locale_path(LocalePrototypeKind::Item, &locale),
        )
        .unwrap();
    let reopened = store.open(&profile_name("warnings")).unwrap();

    assert_eq!(created.warning_count(), 1);
    assert_eq!(reopened.warning_count(), 1);
    assert_eq!(created.diagnostics(), reopened.diagnostics());
    assert_eq!(
        reopened.diagnostics()[0].severity,
        DiagnosticSeverity::Warning
    );
    assert_eq!(reopened.metadata().data_source().path(), data.as_path());
    assert_eq!(reopened.metadata().locale_sources().len(), 1);
    assert!(
        reopened.metadata().imported_at_unix_seconds() > 0,
        "imports should record their wall-clock time"
    );
    assert_eq!(
        reopened
            .catalog()
            .commodity(&CommodityId::Item(ItemId::new("iron-plate").unwrap()))
            .unwrap()
            .display_name(),
        "Iron plate"
    );
}

#[test]
fn round_trips_every_normalized_catalog_record_and_rebuilds_indexes() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(
        &source_directory,
        "catalog.json",
        include_str!("fixtures/localization-data-raw.json"),
    );
    let item_locale = write_data(
        &source_directory,
        "item-locale.json",
        include_str!("fixtures/prototype-locale/item-locale.json"),
    );
    let fluid_locale = write_data(
        &source_directory,
        "fluid-locale.json",
        include_str!("fixtures/prototype-locale/fluid-locale.json"),
    );
    let recipe_locale = write_data(
        &source_directory,
        "recipe-locale.json",
        include_str!("fixtures/prototype-locale/recipe-locale.json"),
    );
    let entity_locale = write_data(
        &source_directory,
        "entity-locale.json",
        include_str!("fixtures/prototype-locale/entity-locale.json"),
    );
    let request = ProfileImportRequest::new(profile_name("complete"), &data)
        .with_locale_path(LocalePrototypeKind::Item, item_locale)
        .with_locale_path(LocalePrototypeKind::Fluid, fluid_locale)
        .with_locale_path(LocalePrototypeKind::Recipe, recipe_locale)
        .with_locale_path(LocalePrototypeKind::Entity, entity_locale);

    let created = store.create(&request).unwrap();
    let reopened = store.open(&profile_name("complete")).unwrap();

    assert_eq!(reopened.catalog(), created.catalog());
    let iron_plate = CommodityId::Item(ItemId::new("iron-plate").unwrap());
    assert_eq!(
        reopened.catalog().recipes_for_product(&iron_plate),
        created.catalog().recipes_for_product(&iron_plate)
    );
    let smelting = factorio_planner_tui::catalog::RecipeCategory::new("smelting").unwrap();
    assert_eq!(
        reopened.catalog().machines_for_category(&smelting),
        created.catalog().machines_for_category(&smelting)
    );
}

#[test]
fn failed_index_write_during_replace_preserves_the_existing_profile() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let original = store
        .create(&ProfileImportRequest::new(profile_name("main"), &data))
        .unwrap();

    fs::write(&data, minimal_data("steel-plate")).unwrap();
    fs::create_dir(store.index_path().with_file_name("profiles.json.tmp")).unwrap();
    assert!(matches!(
        store.replace(&ProfileImportRequest::new(profile_name("main"), &data)),
        Err(ProfileError::Io { .. })
    ));

    let reopened = store.open(&profile_name("main")).unwrap();
    assert_eq!(reopened.fingerprint(), original.fingerprint());
    assert!(
        reopened
            .catalog()
            .commodity(&CommodityId::Item(ItemId::new("iron-plate").unwrap()))
            .is_some()
    );
}

#[test]
fn failed_catalog_write_during_replace_preserves_the_existing_profile() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let original = store
        .create(&ProfileImportRequest::new(profile_name("main"), &data))
        .unwrap();

    fs::write(&data, minimal_data("steel-plate")).unwrap();
    let catalogs_backup = directory.path().join("catalogs-backup");
    fs::rename(store.catalogs_dir(), &catalogs_backup).unwrap();
    fs::write(store.catalogs_dir(), "not a directory").unwrap();
    assert!(matches!(
        store.replace(&ProfileImportRequest::new(profile_name("main"), &data)),
        Err(ProfileError::Io { .. })
    ));
    fs::remove_file(store.catalogs_dir()).unwrap();
    fs::rename(catalogs_backup, store.catalogs_dir()).unwrap();

    let reopened = store.open(&profile_name("main")).unwrap();
    assert_eq!(reopened.fingerprint(), original.fingerprint());
}

#[test]
fn rejects_newer_index_and_catalog_schema_versions() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let created = store
        .create(&ProfileImportRequest::new(profile_name("main"), &data))
        .unwrap();

    let mut index: Value = serde_json::from_slice(&fs::read(store.index_path()).unwrap()).unwrap();
    index["schema_version"] = Value::from(999);
    fs::write(
        store.index_path(),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        store.list(),
        Err(ProfileError::UnsupportedIndexSchema {
            found: 999,
            supported: 1
        })
    ));

    index["schema_version"] = Value::from(1);
    fs::write(
        store.index_path(),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();
    let catalog_path = store.catalog_path(created.fingerprint());
    let mut catalog: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    catalog["schema_version"] = Value::from(999);
    fs::write(catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    assert!(matches!(
        store.open(&profile_name("main")),
        Err(ProfileError::UnsupportedCatalogSchema {
            found: 999,
            supported: CATALOG_SCHEMA_VERSION
        })
    ));
}

#[test]
fn rejects_pre_module_policy_cached_catalogs() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let created = store
        .create(&ProfileImportRequest::new(profile_name("main"), &data))
        .unwrap();
    let catalog_path = store.catalog_path(created.fingerprint());
    let mut catalog: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    catalog["schema_version"] = Value::from(1);
    fs::write(catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();

    assert!(matches!(
        store.open(&profile_name("main")),
        Err(ProfileError::UnsupportedCatalogSchema {
            found: 1,
            supported: CATALOG_SCHEMA_VERSION
        })
    ));
}

#[test]
fn rejects_cached_catalog_records_that_violate_domain_invariants() {
    let directory = TempDir::new().unwrap();
    let source_directory = TempDir::new().unwrap();
    let store = ProfileStore::new(directory.path());
    let data = write_data(&source_directory, "data.json", &minimal_data("iron-plate"));
    let created = store
        .create(&ProfileImportRequest::new(profile_name("main"), &data))
        .unwrap();
    let catalog_path = store.catalog_path(created.fingerprint());
    let mut catalog: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    catalog["catalog"]["commodities"][0]["id"]["id"] = Value::from("");
    fs::write(catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();

    assert!(matches!(
        store.open(&profile_name("main")),
        Err(ProfileError::InvalidCatalog { .. })
    ));
}

#[test]
fn validates_and_normalizes_profile_names() {
    assert_eq!(profile_name("  Main Factory  ").as_str(), "Main Factory");
    assert!(ProfileName::new("").is_err());
    assert!(ProfileName::new("   ").is_err());
    assert!(ProfileName::new("line\nbreak").is_err());
    assert_ne!(profile_name("main"), profile_name("Main"));
}
