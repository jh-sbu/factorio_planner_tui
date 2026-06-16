use factorio_planner_tui::app::{App, Screen};
use factorio_planner_tui::tui::EventContext;

#[test]
fn library_entry_point_exposes_start_state_without_terminal() {
    let app = App::new();

    assert_eq!(app.screen(), Screen::Start);
    assert_eq!(EventContext::from(&app), EventContext::default());
}
