use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use factorio_planner_tui::app::{Action, ExitState, Overlay, WorkspaceView};
use factorio_planner_tui::tui::{EventContext, TranslatedEvent, translate_event};

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
fn resize_events_request_redraw_without_mutating_app_state() {
    assert_eq!(
        translate_event(&Event::Resize(100, 40), EventContext::default()),
        TranslatedEvent::Redraw
    );
}
