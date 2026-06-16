use std::fs;
use std::io;
use std::io::IsTerminal;
use std::path::Path;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;

use crate::app::{Action, App, AppError, ExitState, Overlay, Screen, WorkspaceView};
use crate::persistence::{PlanFileStore, ProfileStore};

const MIN_TERMINAL_WIDTH: u16 = 60;
const MIN_TERMINAL_HEIGHT: u16 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventContext {
    pub overlay_open: bool,
    pub exit_state: ExitState,
    pub workspace_view: WorkspaceView,
}

impl EventContext {
    #[must_use]
    pub fn from_app(app: &App) -> Self {
        Self {
            overlay_open: app.overlay().is_some(),
            exit_state: app.exit_state(),
            workspace_view: app.workspace_view(),
        }
    }
}

impl Default for EventContext {
    fn default() -> Self {
        Self {
            overlay_open: false,
            exit_state: ExitState::Running,
            workspace_view: WorkspaceView::AggregatedTable,
        }
    }
}

impl From<&App> for EventContext {
    fn from(app: &App) -> Self {
        Self::from_app(app)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TranslatedEvent {
    Action(Action),
    Redraw,
    Ignored,
}

#[must_use]
pub fn translate_event(event: &Event, context: EventContext) -> TranslatedEvent {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => translate_key_event(*key, context),
        Event::Resize(_, _) => TranslatedEvent::Redraw,
        _ => TranslatedEvent::Ignored,
    }
}

fn translate_key_event(key: KeyEvent, context: EventContext) -> TranslatedEvent {
    match key.code {
        KeyCode::Char('q' | 'Q') => TranslatedEvent::Action(Action::RequestExit),
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            TranslatedEvent::Action(Action::RequestExit)
        }
        KeyCode::Char('?') => TranslatedEvent::Action(Action::OpenOverlay(Overlay::Help)),
        KeyCode::Esc if context.exit_state == ExitState::WaitingForConfirmation => {
            TranslatedEvent::Action(Action::CancelExit)
        }
        KeyCode::Esc if context.overlay_open => TranslatedEvent::Action(Action::CloseOverlay),
        KeyCode::Char('t' | 'T') => {
            TranslatedEvent::Action(Action::SetWorkspaceView(match context.workspace_view {
                WorkspaceView::AggregatedTable => WorkspaceView::DependencyTree,
                WorkspaceView::DependencyTree => WorkspaceView::AggregatedTable,
            }))
        }
        _ => TranslatedEvent::Ignored,
    }
}

pub trait TerminalSessionOps {
    fn is_interactive(&self) -> bool;
    /// Enables raw input mode.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot enter raw mode.
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    /// Disables raw input mode.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot leave raw mode.
    fn disable_raw_mode(&mut self) -> io::Result<()>;
    /// Enters the alternate screen.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot enter the
    /// alternate screen.
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    /// Leaves the alternate screen.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot leave the
    /// alternate screen.
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
    /// Requests keyboard event-type reporting.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot write the request.
    fn push_keyboard_enhancement_flags(&mut self) -> io::Result<()>;
    /// Restores the previous keyboard event reporting mode.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the terminal backend cannot write the restore
    /// request.
    fn pop_keyboard_enhancement_flags(&mut self) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CrosstermSessionOps;

impl TerminalSessionOps for CrosstermSessionOps {
    fn is_interactive(&self) -> bool {
        io::stdout().is_terminal()
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        enable_raw_mode()
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        disable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), EnterAlternateScreen)
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), LeaveAlternateScreen)
    }

    fn push_keyboard_enhancement_flags(&mut self) -> io::Result<()> {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )
    }

    fn pop_keyboard_enhancement_flags(&mut self) -> io::Result<()> {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)
    }
}

#[derive(Debug)]
pub struct TerminalGuard<O: TerminalSessionOps = CrosstermSessionOps> {
    ops: O,
    raw_mode_enabled: bool,
    alternate_screen_entered: bool,
    keyboard_enhancement_pushed: bool,
}

impl<O: TerminalSessionOps> TerminalGuard<O> {
    /// Enters terminal application mode and restores any partial setup if
    /// initialization fails.
    ///
    /// # Errors
    ///
    /// Returns [`TuiError`] when stdout is not interactive or a terminal setup
    /// operation fails.
    pub fn enter(ops: O) -> Result<Self, TuiError> {
        let mut guard = Self {
            ops,
            raw_mode_enabled: false,
            alternate_screen_entered: false,
            keyboard_enhancement_pushed: false,
        };

        if !guard.ops.is_interactive() {
            return Err(TuiError::NotInteractive);
        }

        guard.enable_raw_mode()?;
        if let Err(error) = guard.enter_alternate_screen() {
            let _ = guard.restore();
            return Err(error);
        }
        if let Err(error) = guard.push_keyboard_enhancement_flags() {
            let _ = guard.restore();
            return Err(error);
        }

        Ok(guard)
    }

    /// Restores terminal state. All active restoration steps are attempted even
    /// if an earlier one fails.
    ///
    /// # Errors
    ///
    /// Returns the first restoration error, if any.
    pub fn restore(&mut self) -> Result<(), TuiError> {
        let mut first_error = None;

        if self.keyboard_enhancement_pushed
            && let Err(error) = self.ops.pop_keyboard_enhancement_flags()
        {
            first_error = Some(TuiError::terminal_operation(
                "pop keyboard enhancement flags",
                error,
            ));
        }
        self.keyboard_enhancement_pushed = false;

        if self.alternate_screen_entered
            && let Err(error) = self.ops.leave_alternate_screen()
            && first_error.is_none()
        {
            first_error = Some(TuiError::terminal_operation(
                "leave alternate screen",
                error,
            ));
        }
        self.alternate_screen_entered = false;

        if self.raw_mode_enabled
            && let Err(error) = self.ops.disable_raw_mode()
            && first_error.is_none()
        {
            first_error = Some(TuiError::terminal_operation("disable raw mode", error));
        }
        self.raw_mode_enabled = false;

        first_error.map_or(Ok(()), Err)
    }

    fn enable_raw_mode(&mut self) -> Result<(), TuiError> {
        self.ops
            .enable_raw_mode()
            .map_err(|error| TuiError::terminal_operation("enable raw mode", error))?;
        self.raw_mode_enabled = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> Result<(), TuiError> {
        self.ops
            .enter_alternate_screen()
            .map_err(|error| TuiError::terminal_operation("enter alternate screen", error))?;
        self.alternate_screen_entered = true;
        Ok(())
    }

    fn push_keyboard_enhancement_flags(&mut self) -> Result<(), TuiError> {
        self.ops
            .push_keyboard_enhancement_flags()
            .map_err(|error| {
                TuiError::terminal_operation("push keyboard enhancement flags", error)
            })?;
        self.keyboard_enhancement_pushed = true;
        Ok(())
    }
}

impl<O: TerminalSessionOps> Drop for TerminalGuard<O> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Debug)]
pub struct LoggingGuard {
    _worker_guard: WorkerGuard,
}

impl LoggingGuard {
    #[must_use]
    const fn new(worker_guard: WorkerGuard) -> Self {
        Self {
            _worker_guard: worker_guard,
        }
    }
}

/// Initializes file logging before terminal application mode is entered.
///
/// # Errors
///
/// Returns [`TuiError`] when the log directory cannot be created or the global
/// tracing subscriber cannot be installed.
pub fn initialize_file_logging(profile_root: &Path) -> Result<LoggingGuard, TuiError> {
    let log_directory = profile_root.join("logs");
    fs::create_dir_all(&log_directory).map_err(|error| TuiError::Io {
        operation: "create log directory",
        source: error,
    })?;
    let file_appender =
        tracing_appender::rolling::daily(&log_directory, "factorio-planner-tui.log");
    let (non_blocking, worker_guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).map_err(TuiError::LoggingInit)?;
    Ok(LoggingGuard::new(worker_guard))
}

/// Runs the terminal event loop for an already initialized application state.
///
/// # Errors
///
/// Returns [`TuiError`] for terminal, event, or application dispatch failures.
pub fn run_app(
    app: &mut App,
    profiles: &ProfileStore,
    plans: &PlanFileStore,
) -> Result<(), TuiError> {
    let _guard = TerminalGuard::enter(CrosstermSessionOps)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|error| TuiError::Io {
        operation: "create terminal",
        source: error,
    })?;
    terminal.clear().map_err(|error| TuiError::Io {
        operation: "clear terminal",
        source: error,
    })?;

    loop {
        terminal
            .draw(|frame| render(app, frame))
            .map_err(|error| TuiError::Io {
                operation: "draw frame",
                source: error,
            })?;

        if app.exit_state() == ExitState::Confirmed {
            break;
        }

        let event = event::read().map_err(|error| TuiError::Io {
            operation: "read terminal event",
            source: error,
        })?;
        match translate_event(&event, EventContext::from(&*app)) {
            TranslatedEvent::Action(action) => app.dispatch(action, profiles, plans)?,
            TranslatedEvent::Redraw | TranslatedEvent::Ignored => {}
        }
    }

    Ok(())
}

pub fn render(app: &App, frame: &mut Frame<'_>) {
    let area = frame.area();
    if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
        render_too_small(frame, area);
        return;
    }

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let title = Line::from(vec![
        Span::styled("Factorio Planner", Style::default().fg(Color::Green)),
        Span::raw(format!(" - {}", screen_title(app.screen()))),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::BOTTOM)),
        header,
    );

    match app.screen() {
        Screen::Start => render_start_screen(app, frame, body),
        Screen::Import => render_import_screen(app, frame, body),
        Screen::Profiles => render_profile_screen(app, frame, body),
        Screen::PlanningWorkspace => render_workspace_placeholder(app, frame, body),
        Screen::BlockedPlan => render_blocked_plan(app, frame, body),
    }

    let footer_text = if app.overlay().is_some() {
        "Enter confirm | Esc cancel | q quit"
    } else {
        "Import data | Open plan | ? help | q quit"
    };
    frame.render_widget(Paragraph::new(footer_text), footer);

    if let Some(overlay) = app.overlay() {
        render_overlay(overlay, frame, area);
    }
}

fn render_too_small(frame: &mut Frame<'_>, area: Rect) {
    let lines = vec![
        Line::from("Terminal too small"),
        Line::from(format!("Current: {}x{}", area.width, area.height)),
        Line::from(format!(
            "Minimum: {MIN_TERMINAL_WIDTH}x{MIN_TERMINAL_HEIGHT}"
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Factorio Planner"),
        ),
        area,
    );
}

fn render_start_screen(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).areas(area);

    let commands = vec![
        Line::from("Start"),
        Line::from(""),
        Line::from("Import data"),
        Line::from("Create plan"),
        Line::from("Open plan"),
        Line::from("Manage profiles"),
    ];
    frame.render_widget(
        Paragraph::new(commands).block(Block::default().borders(Borders::ALL).title("Actions")),
        left,
    );

    render_profile_list(app, frame, right, "Profiles");
}

fn render_profile_screen(app: &App, frame: &mut Frame<'_>, area: Rect) {
    render_profile_list(app, frame, area, "Profile Workflows");
}

fn render_profile_list(app: &App, frame: &mut Frame<'_>, area: Rect, title: &'static str) {
    let mut lines = vec![Line::from(format!("Profiles ({})", app.profiles().len()))];
    if app.profiles().is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("No dataset profiles"));
        lines.push(Line::from("Import data to create the first profile."));
    } else {
        for summary in app.profiles() {
            let active_marker = if app
                .active_profile()
                .is_some_and(|profile| profile.name() == summary.name())
            {
                " active"
            } else {
                ""
            };
            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "{}{} - {}",
                summary.name(),
                active_marker,
                pluralize(summary.warning_count(), "warning")
            )));
            lines.push(Line::from(format!(
                "Fingerprint: {}",
                summary.fingerprint().as_str()
            )));
            lines.push(Line::from(format!(
                "Data source: {}",
                summary.metadata().data_source().path().display()
            )));
            lines.push(Line::from(format!(
                "Imported at: {}",
                summary.metadata().imported_at_unix_seconds()
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_import_screen(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let mut lines = vec![Line::from("Import Dataset"), Line::from("")];
    if let Some(pending) = app.pending_import() {
        lines.push(Line::from(format!(
            "Data: {}",
            pending.data_path().display()
        )));
        let locale = pending
            .locale_path()
            .map_or_else(|| "none".to_owned(), |path| path.display().to_string());
        lines.push(Line::from(format!("Locale: {locale}")));
        let profile = pending
            .profile()
            .map_or_else(|| "prompt required".to_owned(), ToString::to_string);
        lines.push(Line::from(format!("Profile: {profile}")));
        lines.push(Line::from(""));
        lines.push(Line::from("Ready to import"));
    } else if let Some(status) = app.status_message() {
        lines.push(Line::from(status.to_owned()));
    } else {
        lines.push(Line::from("No import is pending"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Import")),
        area,
    );
}

fn render_workspace_placeholder(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let plan_name = app
        .plan()
        .map_or_else(|| "No plan".to_owned(), |plan| plan.name().to_string());
    let lines = vec![
        Line::from("Planning Workspace"),
        Line::from(format!("Plan: {plan_name}")),
        Line::from(format!("Workspace view: {:?}", app.workspace_view())),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Workspace")),
        area,
    );
}

fn render_blocked_plan(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let mut lines = vec![Line::from("Plan blocked")];
    if let Some(blocked) = app.blocked_plan() {
        lines.push(Line::from(format!("Plan: {}", blocked.document().name())));
        lines.push(Line::from(format!("Reason: {:?}", blocked.reason())));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Dataset Mismatch"),
        ),
        area,
    );
}

fn render_overlay(overlay: &Overlay, frame: &mut Frame<'_>, area: Rect) {
    let overlay_area = centered_rect(area, 52, 7);
    let lines = match overlay {
        Overlay::ConfirmExit => vec![
            Line::from("Discard unsaved changes and exit?"),
            Line::from("Enter confirm"),
            Line::from("Esc cancel"),
        ],
        Overlay::ConfirmProfileReplace { profile } => vec![
            Line::from(format!("Replace profile {profile}?")),
            Line::from("The existing profile mapping will be replaced."),
            Line::from("Enter confirm"),
            Line::from("Esc cancel"),
        ],
        Overlay::ConfirmProfileDelete { profile } => vec![
            Line::from(format!("Delete profile {profile}?")),
            Line::from("Plans bound to this dataset may need rebinding."),
            Line::from("Enter confirm"),
            Line::from("Esc cancel"),
        ],
        Overlay::Help => vec![
            Line::from("Help"),
            Line::from("Use the listed commands to manage profiles and plans."),
            Line::from("Esc close"),
        ],
        Overlay::Diagnostics => vec![Line::from("Diagnostics"), Line::from("Esc close")],
        Overlay::Selection(_) => vec![Line::from("Selection"), Line::from("Esc close")],
    };
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Confirm")),
        overlay_area,
    );
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn screen_title(screen: Screen) -> &'static str {
    match screen {
        Screen::Start => "Start",
        Screen::Import => "Import",
        Screen::Profiles => "Profiles",
        Screen::PlanningWorkspace => "Planning",
        Screen::BlockedPlan => "Blocked Plan",
    }
}

fn pluralize(count: usize, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("stdout is not an interactive terminal")]
    NotInteractive,
    #[error("{operation} failed: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("initialize file logging failed: {0}")]
    LoggingInit(#[source] tracing::subscriber::SetGlobalDefaultError),
    #[error(transparent)]
    App(#[from] AppError),
}

impl TuiError {
    fn terminal_operation(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::panic::{self, AssertUnwindSafe};
    use std::rc::Rc;

    use super::{TerminalGuard, TerminalSessionOps};

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum TerminalCall {
        EnableRawMode,
        EnterAlternateScreen,
        PushKeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableRawMode,
    }

    #[derive(Clone, Debug)]
    struct FakeTerminalOps {
        calls: Rc<RefCell<Vec<TerminalCall>>>,
    }

    impl FakeTerminalOps {
        fn new(calls: Rc<RefCell<Vec<TerminalCall>>>) -> Self {
            Self { calls }
        }

        fn record(&self, call: TerminalCall) {
            self.calls.borrow_mut().push(call);
        }
    }

    impl TerminalSessionOps for FakeTerminalOps {
        fn is_interactive(&self) -> bool {
            true
        }

        fn enable_raw_mode(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::EnableRawMode);
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::DisableRawMode);
            Ok(())
        }

        fn enter_alternate_screen(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::EnterAlternateScreen);
            Ok(())
        }

        fn leave_alternate_screen(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::LeaveAlternateScreen);
            Ok(())
        }

        fn push_keyboard_enhancement_flags(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::PushKeyboardEnhancementFlags);
            Ok(())
        }

        fn pop_keyboard_enhancement_flags(&mut self) -> std::io::Result<()> {
            self.record(TerminalCall::PopKeyboardEnhancementFlags);
            Ok(())
        }
    }

    #[test]
    fn terminal_guard_enters_and_restores_terminal_state_in_order() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        {
            let _guard = TerminalGuard::enter(FakeTerminalOps::new(Rc::clone(&calls))).unwrap();
        }

        assert_eq!(
            *calls.borrow(),
            vec![
                TerminalCall::EnableRawMode,
                TerminalCall::EnterAlternateScreen,
                TerminalCall::PushKeyboardEnhancementFlags,
                TerminalCall::PopKeyboardEnhancementFlags,
                TerminalCall::LeaveAlternateScreen,
                TerminalCall::DisableRawMode,
            ]
        );
    }

    #[test]
    fn terminal_guard_restores_terminal_state_during_panic_unwinding() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let result = panic::catch_unwind(AssertUnwindSafe({
            let calls = Rc::clone(&calls);
            move || {
                let _guard = TerminalGuard::enter(FakeTerminalOps::new(calls)).unwrap();
                panic!("forced panic");
            }
        }));

        assert!(result.is_err());
        assert_eq!(
            *calls.borrow(),
            vec![
                TerminalCall::EnableRawMode,
                TerminalCall::EnterAlternateScreen,
                TerminalCall::PushKeyboardEnhancementFlags,
                TerminalCall::PopKeyboardEnhancementFlags,
                TerminalCall::LeaveAlternateScreen,
                TerminalCall::DisableRawMode,
            ]
        );
    }
}
