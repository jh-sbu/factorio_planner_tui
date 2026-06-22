use std::fs;
use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use factorio_planner_tui::app::{
    Action, App, ExitState, FocusTarget, MoveDirection, Overlay, OverlayKind, Screen,
    TextPromptKind, WorkspaceView,
};
use factorio_planner_tui::catalog::{CommodityId, ItemId, MachineId, RecipeId};
use factorio_planner_tui::cli::StartupMode;
use factorio_planner_tui::persistence::{
    PlanDocument, PlanFileStore, PlanName, ProfileImportRequest, ProfileName, ProfileStore,
};
use factorio_planner_tui::planner::{FactoryPlan, Target};
use factorio_planner_tui::tui::{EventContext, TranslatedEvent, render, translate_event};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use tempfile::TempDir;

fn key(code: KeyCode, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind,
        state: KeyEventState::NONE,
    })
}

fn ctrl_key(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn profile_name(name: &str) -> ProfileName {
    ProfileName::new(name).expect("test profile name should be valid")
}

fn plan_name(name: &str) -> PlanName {
    PlanName::new(name).expect("test plan name should be valid")
}

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).expect("test item ID should be valid"))
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

fn warning_data() -> &'static str {
    r#"{
        "item": {
            "iron-ore": {"type": "item", "name": "iron-ore"}
        },
        "assembling-machine": {
            "solar-assembler": {
                "type": "assembling-machine",
                "name": "solar-assembler",
                "crafting_categories": ["crafting"],
                "crafting_speed": 1,
                "energy_usage": "90kW",
                "energy_source": {"type": "solar"}
            }
        }
    }"#
}

fn render_to_string(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(app, frame)).unwrap();
    buffer_to_string(terminal.backend().buffer())
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_owned());
    }
    lines.join("\n")
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

fn workspace_data() -> &'static str {
    r#"{
        "item": {
            "iron-ore": {"type": "item", "name": "iron-ore"},
            "iron-plate": {"type": "item", "name": "iron-plate"},
            "iron-gear-wheel": {"type": "item", "name": "iron-gear-wheel"},
            "pipe": {"type": "item", "name": "pipe"}
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
            },
            "iron-gear-wheel": {
                "type": "recipe",
                "name": "iron-gear-wheel",
                "category": "crafting",
                "energy_required": 0.5,
                "ingredients": [{"type": "item", "name": "iron-plate", "amount": 2}],
                "results": [{"type": "item", "name": "iron-gear-wheel", "amount": 1}]
            },
            "pipe": {
                "type": "recipe",
                "name": "pipe",
                "category": "crafting",
                "energy_required": 0.5,
                "ingredients": [{"type": "item", "name": "iron-plate", "amount": 1}],
                "results": [{"type": "item", "name": "pipe", "amount": 1}]
            }
        },
        "assembling-machine": {
            "assembler": {
                "type": "assembling-machine",
                "name": "assembler",
                "crafting_categories": ["crafting"],
                "crafting_speed": 1,
                "module_slots": 2,
                "energy_usage": "90kW",
                "energy_source": {"type": "electric", "usage_priority": "secondary-input"}
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

fn cyclic_data() -> &'static str {
    r#"{
        "item": {
            "a": {"type": "item", "name": "a"},
            "b": {"type": "item", "name": "b"}
        },
        "recipe": {
            "make-a": {
                "type": "recipe", "name": "make-a", "category": "crafting",
                "ingredients": [{"type": "item", "name": "b", "amount": 1}],
                "results": [{"type": "item", "name": "a", "amount": 1}]
            },
            "make-b": {
                "type": "recipe", "name": "make-b", "category": "crafting",
                "ingredients": [{"type": "item", "name": "a", "amount": 1}],
                "results": [{"type": "item", "name": "b", "amount": 1}]
            }
        },
        "assembling-machine": {
            "assembler": {
                "type": "assembling-machine", "name": "assembler",
                "crafting_categories": ["crafting"], "crafting_speed": 1,
                "energy_usage": "90kW",
                "energy_source": {"type": "electric", "usage_priority": "secondary-input"}
            }
        }
    }"#
}

#[test]
fn key_translation_ignores_non_press_events() {
    let context = EventContext::default();

    assert_eq!(
        translate_event(&key(KeyCode::Char('q'), KeyEventKind::Repeat), context),
        TranslatedEvent::Ignored
    );
    assert_eq!(
        translate_event(&key(KeyCode::Char('q'), KeyEventKind::Release), context),
        TranslatedEvent::Ignored
    );
}

#[test]
fn key_translation_maps_quit_and_help_actions() {
    let context = EventContext::default();

    assert_eq!(
        translate_event(&key(KeyCode::Char('q'), KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::RequestExit)
    );
    assert_eq!(
        translate_event(&ctrl_key(KeyCode::Char('c')), context),
        TranslatedEvent::Action(Action::RequestExit)
    );
    assert_eq!(
        translate_event(&key(KeyCode::Char('?'), KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::OpenOverlay(Overlay::Help))
    );
}

#[test]
fn escape_closes_the_active_modal_state() {
    assert_eq!(
        translate_event(
            &key(KeyCode::Esc, KeyEventKind::Press),
            EventContext {
                overlay_open: true,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::CloseOverlay)
    );
    assert_eq!(
        translate_event(
            &key(KeyCode::Esc, KeyEventKind::Press),
            EventContext {
                exit_state: ExitState::WaitingForConfirmation,
                overlay_open: true,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::CancelExit)
    );
    assert_eq!(
        translate_event(
            &key(KeyCode::Esc, KeyEventKind::Press),
            EventContext::default()
        ),
        TranslatedEvent::Ignored
    );
}

#[test]
fn table_tree_toggle_uses_current_workspace_view() {
    assert_eq!(
        translate_event(
            &key(KeyCode::Char('t'), KeyEventKind::Press),
            EventContext::default()
        ),
        TranslatedEvent::Action(Action::SetWorkspaceView(WorkspaceView::DependencyTree))
    );
    assert_eq!(
        translate_event(
            &key(KeyCode::Char('t'), KeyEventKind::Press),
            EventContext {
                workspace_view: WorkspaceView::DependencyTree,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::SetWorkspaceView(WorkspaceView::AggregatedTable))
    );
}

#[test]
fn start_profile_keys_move_activate_and_return() {
    assert_eq!(
        translate_event(
            &key(KeyCode::Char('j'), KeyEventKind::Press),
            EventContext {
                screen: Screen::Start,
                focus: FocusTarget::StartMenu,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::MoveSelection(MoveDirection::Next))
    );
    assert_eq!(
        translate_event(
            &key(KeyCode::Enter, KeyEventKind::Press),
            EventContext {
                screen: Screen::Start,
                focus: FocusTarget::ProfileList,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::ActivateSelection)
    );
    assert_eq!(
        translate_event(
            &key(KeyCode::Esc, KeyEventKind::Press),
            EventContext {
                screen: Screen::Profiles,
                focus: FocusTarget::ProfileList,
                ..EventContext::default()
            },
        ),
        TranslatedEvent::Action(Action::ReturnToStart)
    );
}

#[test]
fn text_prompt_keys_edit_and_submit_without_treating_q_as_quit() {
    let context = EventContext {
        overlay_kind: Some(OverlayKind::TextPrompt),
        ..EventContext::default()
    };

    assert_eq!(
        translate_event(&key(KeyCode::Char('q'), KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::AppendPromptText("q".to_owned()))
    );
    assert_eq!(
        translate_event(&key(KeyCode::Backspace, KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::BackspacePromptText)
    );
    assert_eq!(
        translate_event(&key(KeyCode::Enter, KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::SubmitPrompt)
    );
    assert_eq!(
        translate_event(&ctrl_key(KeyCode::Char('c')), context),
        TranslatedEvent::Action(Action::RequestExit)
    );
}

#[test]
fn selection_overlay_printable_keys_edit_query() {
    let context = EventContext {
        overlay_kind: Some(OverlayKind::Selection),
        ..EventContext::default()
    };

    assert_eq!(
        translate_event(&key(KeyCode::Char('i'), KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::AppendSelectionQuery("i".to_owned()))
    );
    assert_eq!(
        translate_event(&key(KeyCode::Backspace, KeyEventKind::Press), context),
        TranslatedEvent::Action(Action::BackspaceSelectionQuery)
    );
}

#[test]
fn resize_events_request_redraw_without_mutating_app_state() {
    assert_eq!(
        translate_event(&Event::Resize(100, 40), EventContext::default()),
        TranslatedEvent::Redraw
    );
}

#[test]
fn renders_empty_start_screen() {
    let app = App::new();

    let screen = render_to_string(&app, 80, 20);

    assert!(screen.contains("Factorio Planner"));
    assert!(screen.contains("Start"));
    assert!(screen.contains("No dataset profiles"));
    assert!(screen.contains("Import data"));
    assert!(screen.contains("Open plan"));
}

#[test]
fn renders_selectable_cycle_members_with_resolution_keys() {
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
    let app = App::start(StartupMode::OpenPlan { path }, &profile_store, &plan_store).unwrap();

    let screen = render_to_string(&app, 150, 30);

    assert!(screen.contains("Select a cycle member, then use r or x:"));
    assert!(screen.contains("> a"));
    assert!(screen.contains("  b"));
}

#[test]
fn renders_start_screen_with_profile_metadata() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    create_profile(&root, &sources, "main", "full.json", full_data());
    create_profile(&root, &sources, "warnings", "warnings.json", warning_data());
    let app = App::start(
        StartupMode::StartScreen,
        &ProfileStore::new(root.path()),
        &PlanFileStore::new(),
    )
    .unwrap();

    let screen = render_to_string(&app, 100, 24);

    assert!(screen.contains("Profiles (2)"));
    assert!(screen.contains("> Import data"));
    assert!(screen.contains("> main"));
    assert!(screen.contains("main"));
    assert!(screen.contains("active"));
    assert!(screen.contains("warnings"));
    assert!(screen.contains("1 warning"));
    assert!(screen.contains("Data source:"));
    assert!(screen.contains("full.json"));
}

#[test]
fn renders_text_prompt_overlay() {
    let mut app = App::new();
    app.dispatch(
        Action::OpenOverlay(Overlay::TextPrompt(TextPromptKind::PlanName)),
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();
    app.dispatch(
        Action::AppendPromptText("Starter Base".to_owned()),
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();

    let screen = render_to_string(&app, 80, 20);

    assert!(screen.contains("Plan name"));
    assert!(screen.contains("Starter Base"));
    assert!(screen.contains("Enter confirm"));
}

#[test]
fn renders_pending_import_paths_and_profile_name() {
    let app = App::start(
        StartupMode::ImportData {
            data_path: PathBuf::from("/tmp/data.raw.json"),
            locale_path: Some(PathBuf::from("/tmp/locale.json")),
            profile: Some(profile_name("modded")),
        },
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();

    let screen = render_to_string(&app, 100, 20);

    assert!(screen.contains("Import Dataset"));
    assert!(screen.contains("/tmp/data.raw.json"));
    assert!(screen.contains("/tmp/locale.json"));
    assert!(screen.contains("Profile: modded"));
    assert!(screen.contains("Ready to import"));
}

#[test]
fn renders_import_success_and_failure_statuses() {
    let mut app = App::new();

    app.dispatch(
        Action::ReportImportSuccess {
            profile: profile_name("main"),
            warning_count: 2,
        },
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();
    let success = render_to_string(&app, 100, 20);
    assert!(success.contains("Imported profile main"));
    assert!(success.contains("2 warnings"));

    app.dispatch(
        Action::ReportImportFailure {
            message: "invalid JSON at line 4".to_owned(),
        },
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();
    let failure = render_to_string(&app, 100, 20);
    assert!(failure.contains("Import failed"));
    assert!(failure.contains("invalid JSON at line 4"));
}

#[test]
fn renders_profile_confirmation_overlays() {
    let mut app = App::new();
    app.dispatch(
        Action::OpenOverlay(Overlay::ConfirmProfileReplace {
            profile: profile_name("main"),
        }),
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();

    let replace = render_to_string(&app, 80, 20);
    assert!(replace.contains("Replace profile main?"));
    assert!(replace.contains("Enter confirm"));

    app.dispatch(
        Action::OpenOverlay(Overlay::ConfirmProfileDelete {
            profile: profile_name("main"),
        }),
        &ProfileStore::new(TempDir::new().unwrap().path()),
        &PlanFileStore::new(),
    )
    .unwrap();

    let delete = render_to_string(&app, 80, 20);
    assert!(delete.contains("Delete profile main?"));
    assert!(delete.contains("Esc cancel"));
}

#[test]
fn renders_planning_workspace_table_and_selected_step_details() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "workspace.json", workspace_data());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut plan = FactoryPlan::new(target(item("iron-gear-wheel"), 3.0));
    plan.add_target(target(item("pipe"), 4.0));
    plan.set_selected_belt(factorio_planner_tui::catalog::BeltId::new("transport-belt").unwrap());
    let mut document = PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        plan,
    );
    plan_store.save(&path, &mut document).unwrap();
    let app = App::start(
        StartupMode::OpenPlan { path },
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();

    let screen = render_to_string(&app, 120, 32);

    assert!(screen.contains("Starter Base"));
    assert!(screen.contains("Targets"));
    assert!(screen.contains("Aggregated Table"));
    assert!(screen.contains("iron-gear-wheel"));
    assert!(screen.contains("iron-plate"));
    assert!(screen.contains("assembler"));
    assert!(screen.contains("Selected Step"));
    assert!(screen.contains("External Inputs"));
    assert!(screen.contains("iron-ore"));
    assert!(screen.contains("Belt"));
}

#[test]
fn renders_dependency_tree_with_shared_and_external_labels() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "workspace.json", workspace_data());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut plan = FactoryPlan::new(target(item("iron-gear-wheel"), 3.0));
    plan.add_target(target(item("pipe"), 4.0));
    let mut document = PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        plan,
    );
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(
        StartupMode::OpenPlan { path },
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::SetWorkspaceView(WorkspaceView::DependencyTree),
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();

    let screen = render_to_string(&app, 120, 32);

    assert!(screen.contains("Dependency Tree"));
    assert!(screen.contains("[shared]"));
    assert!(screen.contains("[external]"));
    assert!(screen.contains("iron-plate"));
}

#[test]
fn renders_selection_and_diagnostics_overlays_from_workspace_state() {
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let profile = create_profile(&root, &sources, "main", "workspace.json", workspace_data());
    let plan_store = PlanFileStore::new();
    let path = root.path().join("starter.fptplan.json");
    let mut document = PlanDocument::new(
        plan_name("Starter Base"),
        profile.name().clone(),
        profile.fingerprint().clone(),
        FactoryPlan::new(target(item("iron-plate"), 2.0)),
    );
    plan_store.save(&path, &mut document).unwrap();
    let mut app = App::start(
        StartupMode::OpenPlan { path },
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::OpenOverlay(Overlay::Selection(
            factorio_planner_tui::app::SelectionKind::Recipe {
                commodity: item("iron-plate"),
            },
        )),
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::SetSelectionQuery("advanced".to_owned()),
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();

    let selection = render_to_string(&app, 100, 28);
    assert!(selection.contains("Selection: Recipe"));
    assert!(selection.contains("Query: advanced"));
    assert!(selection.contains("advanced-iron-plate"));

    app.dispatch(
        Action::SetMachineChoice {
            recipe: recipe_id("iron-plate"),
            machine: machine_id("missing-machine"),
        },
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();
    app.dispatch(
        Action::OpenOverlay(Overlay::Diagnostics),
        &ProfileStore::new(root.path()),
        &plan_store,
    )
    .unwrap();
    let diagnostics = render_to_string(&app, 100, 28);
    assert!(diagnostics.contains("Diagnostics"));
    assert!(diagnostics.contains("missing-machine"));
}

#[test]
fn renders_narrow_terminal_fallback() {
    let app = App::new();

    let screen = render_to_string(&app, 30, 6);

    assert!(screen.contains("Terminal too small"));
    assert!(screen.contains("Current: 30x6"));
    assert!(screen.contains("Minimum:"));
}
