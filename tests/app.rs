use std::fs;
use std::path::PathBuf;

use factorio_planner_tui::app::{
    Action, App, ExitState, MoveDirection, Overlay, Screen, SelectionKind, WorkspaceView,
};
use factorio_planner_tui::catalog::{CommodityId, FluidId, ItemId, MachineId, RecipeId};
use factorio_planner_tui::cli::{StartupInputError, StartupMode, StartupRequest};
use factorio_planner_tui::persistence::{
    PlanDocument, PlanFileStore, PlanName, ProfileImportRequest, ProfileName, ProfileStore,
};
use factorio_planner_tui::planner::{FactoryPlan, PlannerError, Target};
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

fn machine_id(name: &str) -> MachineId {
    MachineId::new(name).expect("test machine ID should be valid")
}

fn target(commodity: CommodityId, rate_per_second: f64) -> Target {
    Target::new(commodity, rate_per_second).expect("test target should be valid")
}

fn write_data(directory: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn full_data() -> &'static str {
    r#"{
        "item": {
            "iron-ore": {"type": "item", "name": "iron-ore"},
            "iron-plate": {"type": "item", "name": "iron-plate"}
        },
        "recipe": {
            "advanced-iron-plate": {
                "type": "recipe",
                "name": "advanced-iron-plate",
                "category": "crafting",
                "hidden": true,
                "energy_required": 1,
                "ingredients": [],
                "results": [{"type": "item", "name": "iron-plate", "amount": 2}]
            },
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

fn cyclic_data() -> &'static str {
    r#"{
        "item": {
            "a": {"type": "item", "name": "a"},
            "b": {"type": "item", "name": "b"}
        },
        "recipe": {
            "make-a": {
                "type": "recipe",
                "name": "make-a",
                "category": "crafting",
                "ingredients": [{"type": "item", "name": "b", "amount": 1}],
                "results": [{"type": "item", "name": "a", "amount": 1}]
            },
            "make-b": {
                "type": "recipe",
                "name": "make-b",
                "category": "crafting",
                "ingredients": [{"type": "item", "name": "a", "amount": 1}],
                "results": [{"type": "item", "name": "b", "amount": 1}]
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

fn water_source_data() -> &'static str {
    r#"{
        "fluid": {
            "water": {
                "type": "fluid",
                "name": "water",
                "default_temperature": 15,
                "heat_capacity": "0.2kJ"
            }
        },
        "offshore-pump": {
            "offshore-pump": {
                "type": "offshore-pump",
                "name": "offshore-pump",
                "fluid": "water",
                "pumping_speed": 20
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

fn sample_document(profile: &factorio_planner_tui::persistence::DatasetProfile) -> PlanDocument {
    PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        FactoryPlan::new(target(item("iron-plate"), 2.0)),
    )
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1.0e-10,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn startup_request_resolves_modes_and_rejects_conflicts() {
    assert_eq!(
        StartupRequest::default().resolve().unwrap(),
        StartupMode::StartScreen
    );

    assert_eq!(
        StartupRequest::default()
            .with_dataset(profile_name("main"))
            .resolve()
            .unwrap(),
        StartupMode::OpenDataset {
            profile: profile_name("main"),
        }
    );

    assert_eq!(
        StartupRequest::default()
            .with_import_data("data.raw.json")
            .with_locale("locale.json")
            .with_profile(profile_name("new-profile"))
            .resolve()
            .unwrap(),
        StartupMode::ImportData {
            data_path: PathBuf::from("data.raw.json"),
            locale_path: Some(PathBuf::from("locale.json")),
            profile: Some(profile_name("new-profile")),
        }
    );

    assert_eq!(
        StartupRequest::default()
            .with_plan("starter.fptplan.json")
            .resolve()
            .unwrap(),
        StartupMode::OpenPlan {
            path: PathBuf::from("starter.fptplan.json"),
        }
    );

    assert_eq!(
        StartupRequest::default()
            .with_locale("locale.json")
            .resolve(),
        Err(StartupInputError::LocaleRequiresImportData)
    );
    assert_eq!(
        StartupRequest::default()
            .with_profile(profile_name("main"))
            .resolve(),
        Err(StartupInputError::ProfileRequiresImportData)
    );
    assert_eq!(
        StartupRequest::default()
            .with_plan("starter.fptplan.json")
            .with_dataset(profile_name("main"))
            .resolve(),
        Err(StartupInputError::PlanConflictsWithDatasetSelection)
    );
}

#[test]
fn startup_can_select_a_dataset_without_opening_a_workspace() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());

    let app = App::start(
        StartupMode::OpenDataset {
            profile: profile_name("main"),
        },
        &profile_store,
        &PlanFileStore::new(),
    )
    .unwrap();

    assert_eq!(app.screen(), Screen::Start);
    assert_eq!(
        profile_store.active_profile_name().unwrap(),
        Some(profile_name("main"))
    );
    assert_eq!(app.active_profile().unwrap().name(), &profile_name("main"));
    assert!(app.plan().is_none());
    assert!(app.calculation().is_none());
}

#[test]
fn start_and_profile_selections_are_keyboard_driven() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "alpha", "alpha.json", full_data());
    create_profile(&root, &sources, "beta", "beta.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let mut app = App::start(StartupMode::StartScreen, &profile_store, &plan_store).unwrap();

    assert_eq!(app.selected_start_action_index(), 0);
    assert_eq!(app.selected_profile_index(), 0);

    app.dispatch(
        Action::MoveSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.selected_start_action_index(), 1);

    app.dispatch(
        Action::CycleFocus { reverse: false },
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::MoveSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.selected_profile_index(), 1);

    app.dispatch(Action::ActivateSelection, &profile_store, &plan_store)
        .unwrap();

    assert_eq!(app.screen(), Screen::Start);
    assert_eq!(app.active_profile().unwrap().name(), &profile_name("beta"));
    assert_eq!(
        profile_store.active_profile_name().unwrap(),
        Some(profile_name("beta"))
    );
}

#[test]
fn manage_profiles_opens_and_escape_returns_to_start() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let mut app = App::start(StartupMode::StartScreen, &profile_store, &plan_store).unwrap();

    for _ in 0..3 {
        app.dispatch(
            Action::MoveSelection(MoveDirection::Next),
            &profile_store,
            &plan_store,
        )
        .unwrap();
    }
    app.dispatch(Action::ActivateSelection, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(app.screen(), Screen::Profiles);

    app.dispatch(Action::ReturnToStart, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(app.screen(), Screen::Start);
}

#[test]
fn create_plan_prompt_flow_builds_a_workspace() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let mut app = App::start(StartupMode::StartScreen, &profile_store, &plan_store).unwrap();

    app.dispatch(
        Action::MoveSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::ActivateSelection, &profile_store, &plan_store)
        .unwrap();
    assert!(matches!(app.overlay(), Some(Overlay::TextPrompt(_))));

    app.dispatch(
        Action::AppendPromptText("Starter Base".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::SubmitPrompt, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(
        app.overlay(),
        Some(&Overlay::Selection(SelectionKind::Commodity))
    );

    app.dispatch(
        Action::SetSelectionQuery("iron-plate".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::ConfirmSelection, &profile_store, &plan_store)
        .unwrap();
    assert!(matches!(app.overlay(), Some(Overlay::TextPrompt(_))));

    app.dispatch(
        Action::AppendPromptText("2.5".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::SubmitPrompt, &profile_store, &plan_store)
        .unwrap();

    assert_eq!(app.screen(), Screen::PlanningWorkspace);
    let plan = app.plan().unwrap();
    assert_eq!(plan.name(), &plan_name("Starter Base"));
    assert_eq!(plan.dataset_profile(), &profile_name("main"));
    assert!(plan.is_dirty());
    assert_eq!(plan.plan().targets()[0].commodity(), &item("iron-plate"));
    assert_close(plan.plan().targets()[0].rate_per_second().get(), 2.5);
}

#[test]
fn create_plan_prompt_validation_and_cancel_keep_state_safe() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let mut app = App::start(StartupMode::StartScreen, &profile_store, &plan_store).unwrap();

    app.dispatch(
        Action::MoveSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::ActivateSelection, &profile_store, &plan_store)
        .unwrap();
    app.dispatch(Action::SubmitPrompt, &profile_store, &plan_store)
        .unwrap();
    assert!(matches!(app.overlay(), Some(Overlay::TextPrompt(_))));
    assert!(app.status_message().unwrap().contains("plan name"));

    app.dispatch(
        Action::AppendPromptText("Starter Base".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::SubmitPrompt, &profile_store, &plan_store)
        .unwrap();
    app.dispatch(
        Action::SetSelectionQuery("iron-plate".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::ConfirmSelection, &profile_store, &plan_store)
        .unwrap();
    app.dispatch(
        Action::AppendPromptText("0".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::SubmitPrompt, &profile_store, &plan_store)
        .unwrap();
    assert!(matches!(app.overlay(), Some(Overlay::TextPrompt(_))));
    assert!(app.status_message().unwrap().contains("positive"));

    app.dispatch(Action::CancelPrompt, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(app.screen(), Screen::Start);
    assert!(app.overlay().is_none());
    assert!(app.plan().is_none());
}

#[test]
fn opens_a_ready_plan_and_recalculates_after_action_edits() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    plan_store.save(&path, &mut document).unwrap();

    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    assert_eq!(app.screen(), Screen::PlanningWorkspace);
    assert!(!app.plan().unwrap().is_dirty());
    assert_eq!(app.workspace_view(), WorkspaceView::AggregatedTable);
    assert_close(
        app.calculation().unwrap().production_steps()[0]
            .required_output_rate()
            .get(),
        2.0,
    );

    app.dispatch(
        Action::AddTarget(target(item("iron-plate"), 1.0)),
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert!(app.plan().unwrap().is_dirty());
    assert_close(
        app.calculation().unwrap().production_steps()[0]
            .required_output_rate()
            .get(),
        3.0,
    );

    app.dispatch(
        Action::SetWorkspaceView(WorkspaceView::DependencyTree),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.workspace_view(), WorkspaceView::DependencyTree);
}

#[test]
fn calculation_errors_are_state_not_terminal_failures() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    app.dispatch(
        Action::SetMachineChoice {
            recipe: recipe_id("iron-plate"),
            machine: machine_id("missing"),
        },
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert!(app.plan().unwrap().is_dirty());
    assert!(app.calculation().is_none());
    assert_eq!(
        app.calculation_error(),
        Some(&PlannerError::MissingMachineChoice {
            recipe: recipe_id("iron-plate"),
            machine: machine_id("missing"),
        })
    );

    app.dispatch(
        Action::ClearMachineChoice {
            recipe: recipe_id("iron-plate"),
        },
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert!(app.calculation().is_some());
    assert!(app.calculation_error().is_none());
}

#[test]
fn blocked_plan_can_be_rebound_explicitly_to_a_compatible_profile() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let main = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let compatible_path = write_data(&sources, "compatible.json", full_data());
    let compatible = profile_store
        .create(&ProfileImportRequest::new(
            profile_name("compatible"),
            compatible_path,
        ))
        .unwrap();
    assert_eq!(compatible.fingerprint(), main.fingerprint());

    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&main);
    plan_store.save(&path, &mut document).unwrap();

    let minimal_path = write_data(&sources, "minimal.json", &minimal_data("steel-plate"));
    profile_store
        .replace(&ProfileImportRequest::new(
            profile_name("main"),
            minimal_path,
        ))
        .unwrap();

    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    assert_eq!(app.screen(), Screen::BlockedPlan);
    assert!(app.blocked_plan().is_some());
    assert!(app.plan().is_none());

    app.dispatch(
        Action::RebindBlockedPlan {
            profile: profile_name("compatible"),
        },
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert_eq!(app.screen(), Screen::PlanningWorkspace);
    assert_eq!(
        app.active_profile().unwrap().name(),
        &profile_name("compatible")
    );
    assert!(app.blocked_plan().is_none());
    assert!(app.plan().unwrap().is_dirty());
    assert!(app.calculation().is_some());
}

#[test]
fn workspace_selection_and_selector_confirmation_drive_plan_edits() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    document.edit_plan(|plan| plan.add_target(target(item("iron-plate"), 1.0)));
    plan_store.save(&path, &mut document).unwrap();

    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    assert_eq!(app.selected_target_index(), 0);
    assert_eq!(app.selected_result_index(), 0);
    app.dispatch(
        Action::MoveWorkspaceSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.selected_target_index(), 1);

    app.dispatch(
        Action::OpenOverlay(Overlay::Selection(SelectionKind::Recipe {
            commodity: item("iron-plate"),
        })),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::SetSelectionQuery("advanced".to_owned()),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.selection_query(), "advanced");
    app.dispatch(Action::ConfirmSelection, &profile_store, &plan_store)
        .unwrap();

    assert!(app.overlay().is_none());
    assert_eq!(
        app.plan()
            .unwrap()
            .plan()
            .recipe_choice(&item("iron-plate")),
        Some(&recipe_id("advanced-iron-plate"))
    );
    assert!(app.plan().unwrap().is_dirty());
}

#[test]
fn cycle_error_members_remain_selectable_for_recipe_changes() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "cycle.json", cyclic_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("cycle.fptplan.json");
    let mut document = PlanDocument::new(
        plan_name("Cycle"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        FactoryPlan::new(target(item("a"), 1.0)),
    );
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();
    assert!(matches!(
        app.calculation_error(),
        Some(PlannerError::Cycle { .. })
    ));

    app.dispatch(
        Action::MoveFocus(factorio_planner_tui::app::FocusTarget::Results),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::OpenRecipeSelectionForSelected,
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(
        app.overlay(),
        Some(&Overlay::Selection(SelectionKind::Recipe {
            commodity: item("a")
        }))
    );

    app.dispatch(Action::CloseOverlay, &profile_store, &plan_store)
        .unwrap();
    app.dispatch(
        Action::MoveWorkspaceSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::OpenRecipeSelectionForSelected,
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(
        app.overlay(),
        Some(&Overlay::Selection(SelectionKind::Recipe {
            commodity: item("b")
        }))
    );
}

#[test]
fn non_recipe_results_are_selectable_without_recipe_choices() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "water.json", water_source_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("water.fptplan.json");
    let mut document = PlanDocument::new(
        plan_name("Water"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        FactoryPlan::new(target(fluid("water"), 60.0)),
    );
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    app.dispatch(
        Action::MoveFocus(factorio_planner_tui::app::FocusTarget::Results),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::OpenRecipeSelectionForSelected,
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert_eq!(app.selected_result_index(), 0);
    assert!(app.overlay().is_none());
    assert_eq!(
        app.status_message(),
        Some("No recipe choices available for water")
    );
}

#[test]
fn workspace_selection_clamps_after_target_removal() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    document.edit_plan(|plan| plan.add_target(target(item("iron-plate"), 1.0)));
    plan_store.save(&path, &mut document).unwrap();

    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();
    app.dispatch(
        Action::MoveWorkspaceSelection(MoveDirection::Next),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    assert_eq!(app.selected_target_index(), 1);

    app.dispatch(
        Action::RemoveTarget { index: 1 },
        &profile_store,
        &plan_store,
    )
    .unwrap();

    assert_eq!(app.selected_target_index(), 0);
    assert_eq!(app.selected_result_index(), 0);
}

#[test]
fn dirty_exit_requires_confirmation_and_can_be_cancelled() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "full.json", full_data());
    let profile_store = ProfileStore::new(root.path());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = sample_document(&profile);
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    app.dispatch(
        Action::AddTarget(target(item("iron-plate"), 1.0)),
        &profile_store,
        &plan_store,
    )
    .unwrap();
    app.dispatch(Action::RequestExit, &profile_store, &plan_store)
        .unwrap();

    assert_eq!(app.exit_state(), ExitState::WaitingForConfirmation);
    assert_eq!(app.overlay(), Some(&Overlay::ConfirmExit));

    app.dispatch(Action::CancelExit, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(app.exit_state(), ExitState::Running);
    assert!(app.overlay().is_none());

    app.dispatch(Action::RequestExit, &profile_store, &plan_store)
        .unwrap();
    app.dispatch(Action::ConfirmExit, &profile_store, &plan_store)
        .unwrap();
    assert_eq!(app.exit_state(), ExitState::Confirmed);
}
