use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use clap::Parser;
use factorio_planner_tui::app::Screen;
use factorio_planner_tui::catalog::{CommodityId, ItemId};
use factorio_planner_tui::cli::{CliArgs, StartupInputError, StartupMode};
use factorio_planner_tui::persistence::{
    PlanDocument, PlanFileStore, PlanName, ProfileImportRequest, ProfileName, ProfileStore,
};
use factorio_planner_tui::planner::{FactoryPlan, Target};
use factorio_planner_tui::{RunError, run_with_startup_mode};
use predicates::str::contains;
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

fn target(commodity: CommodityId, rate_per_second: f64) -> Target {
    Target::new(commodity, rate_per_second).expect("test target should be valid")
}

fn write_data(directory: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn minimal_data() -> &'static str {
    r#"{
        "item": {
            "iron-ore": {"type": "item", "name": "iron-ore"},
            "iron-plate": {"type": "item", "name": "iron-plate"}
        },
        "recipe": {
            "iron-plate": {
                "type": "recipe",
                "name": "iron-plate",
                "category": "crafting",
                "energy_required": 1,
                "ingredients": [{"type": "item", "name": "iron-ore", "amount": 1}],
                "results": [{"type": "item", "name": "iron-plate", "amount": 1}]
            }
        },
        "assembling-machine": {
            "assembler": {
                "type": "assembling-machine",
                "name": "assembler",
                "crafting_categories": ["crafting"],
                "crafting_speed": 1,
                "energy_usage": "90kW",
                "energy_source": {"type": "electric", "usage_priority": "secondary-input"}
            }
        }
    }"#
}

fn create_profile(
    root: &TempDir,
    sources: &TempDir,
    name: &str,
) -> factorio_planner_tui::persistence::DatasetProfile {
    let store = ProfileStore::new(root.path());
    let data_path = write_data(sources, "data.raw.json", minimal_data());
    store
        .create(&ProfileImportRequest::new(profile_name(name), &data_path))
        .unwrap()
}

fn parse_mode(args: &[&str]) -> Result<StartupMode, StartupInputError> {
    CliArgs::try_parse_from(args)
        .unwrap()
        .into_startup_request()
        .resolve()
}

#[test]
fn parses_documented_startup_modes() {
    assert_eq!(
        parse_mode(&["factorio_planner_tui"]).unwrap(),
        StartupMode::StartScreen
    );
    assert_eq!(
        parse_mode(&["factorio_planner_tui", "--dataset", "main"]).unwrap(),
        StartupMode::OpenDataset {
            profile: profile_name("main"),
        }
    );
    assert_eq!(
        parse_mode(&[
            "factorio_planner_tui",
            "--import-data",
            "data.raw.json",
            "--locale",
            "locale-dir",
            "--profile",
            "modded",
        ])
        .unwrap(),
        StartupMode::ImportData {
            data_path: PathBuf::from("data.raw.json"),
            locale_path: Some(PathBuf::from("locale-dir")),
            profile: Some(profile_name("modded")),
        }
    );
    assert_eq!(
        parse_mode(&["factorio_planner_tui", "--plan", "starter.fptplan.json"]).unwrap(),
        StartupMode::OpenPlan {
            path: PathBuf::from("starter.fptplan.json"),
        }
    );
}

#[test]
fn rejects_invalid_cli_argument_combinations_and_profile_names() {
    assert_eq!(
        parse_mode(&["factorio_planner_tui", "--locale", "locale-dir"]),
        Err(StartupInputError::LocaleRequiresImportData)
    );
    assert_eq!(
        parse_mode(&["factorio_planner_tui", "--profile", "main"]),
        Err(StartupInputError::ProfileRequiresImportData)
    );
    assert_eq!(
        parse_mode(&[
            "factorio_planner_tui",
            "--plan",
            "starter.fptplan.json",
            "--dataset",
            "main",
        ]),
        Err(StartupInputError::PlanConflictsWithDatasetSelection)
    );

    assert!(
        CliArgs::try_parse_from(["factorio_planner_tui", "--dataset", ""]).is_err(),
        "empty profile names should fail during argument parsing"
    );
}

#[test]
fn run_imports_named_profile_before_launching_the_tui() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let data_path = write_data(&sources, "data.raw.json", minimal_data());
    let profiles = ProfileStore::new(root.path());
    let plans = PlanFileStore::new();
    let mut launched = false;

    run_with_startup_mode(
        StartupMode::ImportData {
            data_path,
            locale_path: None,
            profile: Some(profile_name("main")),
        },
        &profiles,
        &plans,
        |app, profiles, _plans| {
            launched = true;
            assert_eq!(app.screen(), Screen::Import);
            assert!(
                app.status_message()
                    .unwrap()
                    .contains("Imported profile main")
            );
            assert_eq!(
                profiles.active_profile_name().unwrap(),
                Some(profile_name("main"))
            );
            assert_eq!(app.active_profile().unwrap().name(), &profile_name("main"));
            Ok(())
        },
    )
    .unwrap();

    assert!(launched);
    assert!(profiles.open(&profile_name("main")).is_ok());
}

#[test]
fn run_imports_locale_directory_when_present() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let locale_dir = TempDir::new().unwrap();
    let data_path = write_data(&sources, "data.raw.json", minimal_data());
    fs::write(
        locale_dir.path().join("item-locale.json"),
        r#"{"names":{"iron-plate":"Iron plate"}}"#,
    )
    .unwrap();
    let profiles = ProfileStore::new(root.path());
    let plans = PlanFileStore::new();

    run_with_startup_mode(
        StartupMode::ImportData {
            data_path,
            locale_path: Some(locale_dir.path().to_path_buf()),
            profile: Some(profile_name("localized")),
        },
        &profiles,
        &plans,
        |app, _profiles, _plans| {
            let catalog = app.active_profile().unwrap().catalog();
            assert_eq!(
                catalog
                    .commodity(&item("iron-plate"))
                    .unwrap()
                    .display_name(),
                "Iron plate"
            );
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn named_import_fails_before_tui_launch_when_profile_exists_or_locale_is_empty() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main");
    let data_path = write_data(&sources, "duplicate.raw.json", minimal_data());
    let profiles = ProfileStore::new(root.path());
    let plans = PlanFileStore::new();

    let duplicate = run_with_startup_mode(
        StartupMode::ImportData {
            data_path,
            locale_path: None,
            profile: Some(profile_name("main")),
        },
        &profiles,
        &plans,
        |_app, _profiles, _plans| panic!("TUI should not launch after import failure"),
    );
    assert!(matches!(
        duplicate,
        Err(RunError::Profile(
            factorio_planner_tui::persistence::ProfileError::ProfileAlreadyExists { .. }
        ))
    ));

    let data_path = write_data(&sources, "empty-locale.raw.json", minimal_data());
    let locale_dir = TempDir::new().unwrap();
    let empty_locale = run_with_startup_mode(
        StartupMode::ImportData {
            data_path,
            locale_path: Some(locale_dir.path().to_path_buf()),
            profile: Some(profile_name("other")),
        },
        &profiles,
        &plans,
        |_app, _profiles, _plans| panic!("TUI should not launch after import failure"),
    );
    assert!(matches!(
        empty_locale,
        Err(RunError::EmptyLocaleDirectory { .. })
    ));
}

#[test]
fn run_opens_plan_startup_modes_through_the_same_app_state() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main");
    let profiles = ProfileStore::new(root.path());
    let plans = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        FactoryPlan::new(target(item("iron-plate"), 2.0)),
    );
    plans.save(&path, &mut document).unwrap();

    run_with_startup_mode(
        StartupMode::OpenPlan { path },
        &profiles,
        &plans,
        |app, _profiles, _plans| {
            assert_eq!(app.screen(), Screen::PlanningWorkspace);
            assert_eq!(app.plan().unwrap().name(), &plan_name("Starter Base"));
            assert!(app.calculation().is_some());
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn binary_reports_help_and_pre_tui_errors() {
    Command::cargo_bin("factorio_planner_tui")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--import-data"))
        .stdout(contains("--dataset"))
        .stdout(contains("--plan"));

    Command::cargo_bin("factorio_planner_tui")
        .unwrap()
        .args(["--dataset", ""])
        .assert()
        .failure()
        .stderr(contains("profile name must not be empty"));

    Command::cargo_bin("factorio_planner_tui")
        .unwrap()
        .args([
            "--import-data",
            "/definitely/missing/data.raw.json",
            "--profile",
            "missing",
        ])
        .assert()
        .failure()
        .stderr(contains("open import source"));
}
