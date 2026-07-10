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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;

use crate::app::{
    Action, App, AppError, ExitState, FocusTarget, MoveDirection, Overlay, OverlayKind, Screen,
    SelectionKind, TextPromptKind, WorkspaceView,
};
use crate::catalog::{
    BeltId, Catalog, CommodityId, FluidSourceKind, FuelId, MachineId, MiningMachineId, ModuleId,
    Positive, ProductionSource, RecipeId, ResourceSourceId,
};
use crate::import::DiagnosticSeverity;
use crate::persistence::{PlanFileStore, ProfileStore};
use crate::planner::{
    CommodityRate, DependencyNode, DependencyNodeKind, ExtractionStep, RateUnit, StepEnergy,
};

const MIN_TERMINAL_WIDTH: u16 = 60;
const MIN_TERMINAL_HEIGHT: u16 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventContext {
    pub screen: Screen,
    pub overlay_open: bool,
    pub overlay_kind: Option<OverlayKind>,
    pub create_plan_in_progress: bool,
    pub exit_state: ExitState,
    pub workspace_view: WorkspaceView,
    pub focus: FocusTarget,
}

impl EventContext {
    #[must_use]
    pub fn from_app(app: &App) -> Self {
        Self {
            screen: app.screen(),
            overlay_open: app.overlay().is_some(),
            overlay_kind: app.overlay().map(OverlayKind::from),
            create_plan_in_progress: app.create_plan_in_progress(),
            exit_state: app.exit_state(),
            workspace_view: app.workspace_view(),
            focus: app.focus(),
        }
    }
}

impl Default for EventContext {
    fn default() -> Self {
        Self {
            screen: Screen::Start,
            overlay_open: false,
            overlay_kind: None,
            create_plan_in_progress: false,
            exit_state: ExitState::Running,
            workspace_view: WorkspaceView::AggregatedTable,
            focus: FocusTarget::StartMenu,
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
    if matches!(key.code, KeyCode::Char('c' | 'C')) && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return TranslatedEvent::Action(Action::RequestExit);
    }
    if context.overlay_kind == Some(OverlayKind::TextPrompt) {
        return translate_text_prompt_key_event(key);
    }
    if context.overlay_kind == Some(OverlayKind::Selection) {
        return translate_selection_key_event(key, context);
    }

    match key.code {
        KeyCode::Char('q' | 'Q') => TranslatedEvent::Action(Action::RequestExit),
        KeyCode::Enter if context.exit_state == ExitState::WaitingForConfirmation => {
            TranslatedEvent::Action(Action::ConfirmExit)
        }
        KeyCode::Enter if context.overlay_open => TranslatedEvent::Action(Action::ConfirmSelection),
        KeyCode::Enter if matches!(context.screen, Screen::Start | Screen::Profiles) => {
            TranslatedEvent::Action(Action::ActivateSelection)
        }
        KeyCode::Char('?') => TranslatedEvent::Action(Action::OpenOverlay(Overlay::Help)),
        KeyCode::Esc if context.exit_state == ExitState::WaitingForConfirmation => {
            TranslatedEvent::Action(Action::CancelExit)
        }
        KeyCode::Esc if context.screen == Screen::Profiles => {
            TranslatedEvent::Action(Action::ReturnToStart)
        }
        KeyCode::Esc if context.overlay_open => TranslatedEvent::Action(Action::CloseOverlay),
        KeyCode::Up | KeyCode::Char('k' | 'K') if context.overlay_open => {
            TranslatedEvent::Action(Action::MoveSelectorSelection(MoveDirection::Previous))
        }
        KeyCode::Down | KeyCode::Char('j' | 'J') if context.overlay_open => {
            TranslatedEvent::Action(Action::MoveSelectorSelection(MoveDirection::Next))
        }
        KeyCode::Up | KeyCode::Char('k' | 'K') => {
            TranslatedEvent::Action(Action::MoveSelection(MoveDirection::Previous))
        }
        KeyCode::Down | KeyCode::Char('j' | 'J') => {
            TranslatedEvent::Action(Action::MoveSelection(MoveDirection::Next))
        }
        KeyCode::Tab => TranslatedEvent::Action(Action::CycleFocus { reverse: false }),
        KeyCode::BackTab => TranslatedEvent::Action(Action::CycleFocus { reverse: true }),
        KeyCode::Char('t' | 'T') => {
            TranslatedEvent::Action(Action::SetWorkspaceView(match context.workspace_view {
                WorkspaceView::AggregatedTable => WorkspaceView::DependencyTree,
                WorkspaceView::DependencyTree => WorkspaceView::AggregatedTable,
            }))
        }
        KeyCode::Char('r' | 'R') => TranslatedEvent::Action(Action::OpenRecipeSelectionForSelected),
        KeyCode::Char('m' | 'M') => {
            TranslatedEvent::Action(Action::OpenMachineSelectionForSelected)
        }
        KeyCode::Char('u' | 'U') => {
            TranslatedEvent::Action(Action::OpenModulesSelectionForSelected)
        }
        KeyCode::Char('f' | 'F') => TranslatedEvent::Action(Action::OpenFuelSelectionForSelected),
        KeyCode::Char('b' | 'B') => {
            TranslatedEvent::Action(Action::OpenOverlay(Overlay::Selection(SelectionKind::Belt)))
        }
        KeyCode::Char('x' | 'X') => TranslatedEvent::Action(Action::ToggleSelectedExternalInput),
        _ => TranslatedEvent::Ignored,
    }
}

fn translate_text_prompt_key_event(key: KeyEvent) -> TranslatedEvent {
    match key.code {
        KeyCode::Enter => TranslatedEvent::Action(Action::SubmitPrompt),
        KeyCode::Esc => TranslatedEvent::Action(Action::CancelPrompt),
        KeyCode::Backspace => TranslatedEvent::Action(Action::BackspacePromptText),
        KeyCode::Char(value) => {
            TranslatedEvent::Action(Action::AppendPromptText(value.to_string()))
        }
        _ => TranslatedEvent::Ignored,
    }
}

fn translate_selection_key_event(key: KeyEvent, context: EventContext) -> TranslatedEvent {
    match key.code {
        KeyCode::Enter => TranslatedEvent::Action(Action::ConfirmSelection),
        KeyCode::Esc if context.create_plan_in_progress => {
            TranslatedEvent::Action(Action::CancelPrompt)
        }
        KeyCode::Esc => TranslatedEvent::Action(Action::CloseOverlay),
        KeyCode::Up => {
            TranslatedEvent::Action(Action::MoveSelectorSelection(MoveDirection::Previous))
        }
        KeyCode::Down => {
            TranslatedEvent::Action(Action::MoveSelectorSelection(MoveDirection::Next))
        }
        KeyCode::Backspace => TranslatedEvent::Action(Action::BackspaceSelectionQuery),
        KeyCode::Char(value) => {
            TranslatedEvent::Action(Action::AppendSelectionQuery(value.to_string()))
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
        Screen::PlanningWorkspace => render_workspace(app, frame, body),
        Screen::BlockedPlan => render_blocked_plan(app, frame, body),
    }

    let footer_text = if app.overlay().is_some() {
        "Enter confirm | Esc cancel | q quit"
    } else if app.screen() == Screen::PlanningWorkspace {
        "j/k move | Tab focus | t table/tree | r recipe | m machine | u modules | f fuel | b belt | ? help | q quit"
    } else {
        "j/k move | Tab focus | Enter select | ? help | q quit"
    };
    frame.render_widget(Paragraph::new(footer_text), footer);

    if let Some(overlay) = app.overlay() {
        render_overlay(app, overlay, frame, area);
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

fn section_block(title: &'static str, focused: bool) -> Block<'static> {
    let block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        let style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        block
            .border_type(BorderType::Thick)
            .border_style(style)
            .title_style(style)
    } else {
        block
    }
}

fn render_start_screen(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).areas(area);

    let action_labels = ["Import data", "Create plan", "Open plan", "Manage profiles"];
    let mut commands = vec![Line::from("Start"), Line::from("")];
    for (index, label) in action_labels.iter().enumerate() {
        let marker = if app.focus() == FocusTarget::StartMenu
            && index == app.selected_start_action_index()
        {
            ">"
        } else {
            " "
        };
        commands.push(Line::from(format!("{marker} {label}")));
    }
    if let Some(status) = app.status_message() {
        commands.push(Line::from(""));
        commands.push(Line::from(status.to_owned()));
    }
    frame.render_widget(
        Paragraph::new(commands).block(section_block(
            "Actions",
            app.focus() == FocusTarget::StartMenu,
        )),
        left,
    );

    render_profile_list(
        app,
        frame,
        right,
        "Profiles",
        app.focus() == FocusTarget::ProfileList,
    );
}

fn render_profile_screen(app: &App, frame: &mut Frame<'_>, area: Rect) {
    render_profile_list(
        app,
        frame,
        area,
        "Profile Workflows",
        app.focus() == FocusTarget::ProfileList,
    );
}

fn render_profile_list(
    app: &App,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    focused: bool,
) {
    let mut lines = vec![Line::from(format!("Profiles ({})", app.profiles().len()))];
    if app.profiles().is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("No dataset profiles"));
        lines.push(Line::from("Import data to create the first profile."));
    } else {
        for (index, summary) in app.profiles().iter().enumerate() {
            let marker = if index == app.selected_profile_index() {
                ">"
            } else {
                " "
            };
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
                "{marker} {}{} - {}",
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
            .block(section_block(title, focused)),
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

fn render_workspace(app: &App, frame: &mut Frame<'_>, area: Rect) {
    if app.plan().is_none() {
        frame.render_widget(
            Paragraph::new("No plan is open")
                .block(Block::default().borders(Borders::ALL).title("Workspace")),
            area,
        );
        return;
    }

    let [summary, body] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(area);
    render_workspace_summary(app, frame, summary);

    if body.width >= 100 {
        let [targets, results, details] = Layout::horizontal([
            Constraint::Percentage(28),
            Constraint::Percentage(44),
            Constraint::Percentage(28),
        ])
        .areas(body);
        render_targets_pane(app, frame, targets);
        render_results_pane(app, frame, results);
        render_details_pane(app, frame, details);
    } else {
        match app.focus() {
            FocusTarget::TargetList => render_targets_pane(app, frame, body),
            FocusTarget::StepConfiguration => render_details_pane(app, frame, body),
            FocusTarget::Results | FocusTarget::StartMenu | FocusTarget::ProfileList => {
                render_results_pane(app, frame, body);
            }
        }
    }
}

fn render_workspace_summary(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(plan) = app.plan() else {
        return;
    };
    let dirty = if plan.is_dirty() { "dirty" } else { "saved" };
    let profile = app.active_profile().map_or_else(
        || plan.dataset_profile().to_string(),
        |profile| profile.name().to_string(),
    );
    let lines = vec![
        Line::from(format!(
            "Plan: {} | Dataset: {profile} | State: {dirty}",
            plan.name()
        )),
        Line::from(format!(
            "Fingerprint: {} | Focus: {:?} | View: {:?}",
            plan.dataset_fingerprint().as_str(),
            app.focus(),
            app.workspace_view()
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Planning Workspace"),
        ),
        area,
    );
}

fn render_targets_pane(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let catalog = active_catalog(app);
    let Some(plan) = app.plan() else {
        return;
    };
    let factory_plan = plan.plan();
    let mut lines = vec![Line::from("Targets")];
    for (index, target) in factory_plan.targets().iter().enumerate() {
        let marker = if index == app.selected_target_index() {
            ">"
        } else {
            " "
        };
        lines.push(Line::from(format!(
            "{marker} {} {}",
            commodity_label(catalog, target.commodity()),
            format_rate(target.rate_per_second(), factory_plan.display_rate_unit())
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("External Inputs"));
    if factory_plan.external_inputs().is_empty() {
        lines.push(Line::from(" none"));
    } else {
        for commodity in factory_plan.external_inputs() {
            lines.push(Line::from(format!(
                " x {}",
                commodity_label(catalog, commodity)
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Required External Inputs"));
    if let Some(calculation) = app.calculation() {
        push_rate_lines(
            catalog,
            calculation.external_inputs(),
            factory_plan.display_rate_unit(),
            &mut lines,
        );
    } else {
        lines.push(Line::from(" none"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(section_block(
                "Targets",
                app.focus() == FocusTarget::TargetList,
            )),
        area,
    );
}

fn render_results_pane(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let lines = if let Some(error) = app.calculation_error() {
        let mut lines = vec![
            Line::from("Calculation error"),
            Line::from(error.to_string()),
        ];
        if app.cycle_error_commodities().is_empty() {
            lines.push(Line::from("Open diagnostics for details."));
        } else {
            lines.push(Line::from("Select a cycle member, then use r or x:"));
            let catalog = active_catalog(app);
            for (index, commodity) in app.cycle_error_commodities().iter().enumerate() {
                let marker = if index == app.selected_result_index() {
                    ">"
                } else {
                    " "
                };
                lines.push(Line::from(format!(
                    "{marker} {}",
                    commodity_label(catalog, commodity)
                )));
            }
        }
        lines
    } else if let Some(calculation) = app.calculation() {
        match app.workspace_view() {
            WorkspaceView::AggregatedTable => aggregated_table_lines(app),
            WorkspaceView::DependencyTree => dependency_tree_lines(app, calculation),
        }
    } else {
        vec![Line::from("No calculation result")]
    };

    let title = match app.workspace_view() {
        WorkspaceView::AggregatedTable => "Aggregated Table",
        WorkspaceView::DependencyTree => "Dependency Tree",
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(section_block(title, app.focus() == FocusTarget::Results)),
        area,
    );
}

fn aggregated_table_lines(app: &App) -> Vec<Line<'static>> {
    let catalog = active_catalog(app);
    let Some(calculation) = app.calculation() else {
        return vec![Line::from("No calculation result")];
    };
    let unit = calculation.display_rate_unit();
    let mut lines = vec![Line::from("Aggregated Table")];
    lines.push(Line::from(
        "Product | Rate | Source | Machine | Machines | Energy | Belt",
    ));
    for (index, step) in calculation.production_steps().iter().enumerate() {
        let marker = if index == app.selected_result_index() {
            ">"
        } else {
            " "
        };
        let belt = calculation
            .belt_equivalents()
            .iter()
            .find(|equivalent| equivalent.commodity() == step.planning_product())
            .map_or_else(
                || "-".to_owned(),
                |equivalent| format!("{} belts", format_quantity(equivalent.exact_belts().get())),
            );
        lines.push(Line::from(format!(
            "{marker} {} | {} | {} | {} | {}/{} | {} | {belt}",
            commodity_label(catalog, step.planning_product()),
            format_rate(step.required_output_rate(), unit),
            recipe_label(catalog, step.recipe()),
            machine_label(catalog, step.machine()),
            format_quantity(step.fractional_machine_count().get()),
            step.installed_machine_count(),
            step_energy_summary(step.energy()),
        )));
    }
    let production_count = calculation.production_steps().len();
    for (offset, step) in calculation.extraction_steps().iter().enumerate() {
        let index = production_count + offset;
        let marker = if index == app.selected_result_index() {
            ">"
        } else {
            " "
        };
        let belt = calculation
            .belt_equivalents()
            .iter()
            .find(|equivalent| equivalent.commodity() == step.planning_product())
            .map_or_else(
                || "-".to_owned(),
                |equivalent| format!("{} belts", format_quantity(equivalent.exact_belts().get())),
            );
        lines.push(Line::from(format!(
            "{marker} {} | {} | {} | {} | {} | {} | {belt}",
            commodity_label(catalog, step.planning_product()),
            format_rate(step.required_output_rate(), unit),
            source_label(catalog, step.source()),
            step.mining_machine().map_or_else(
                || "-".to_owned(),
                |miner| mining_machine_label(catalog, miner)
            ),
            extraction_machine_count_summary(step),
            step.energy()
                .map_or_else(|| "-".to_owned(), step_energy_summary),
        )));
    }
    lines
}

fn dependency_tree_lines(
    app: &App,
    calculation: &crate::planner::CalculationResult,
) -> Vec<Line<'static>> {
    let catalog = active_catalog(app);
    let mut lines = vec![Line::from("Dependency Tree")];
    for tree in calculation.dependency_trees() {
        push_dependency_node_lines(catalog, tree, 0, &mut lines);
    }
    lines
}

fn push_dependency_node_lines(
    catalog: Option<&Catalog>,
    node: &DependencyNode,
    depth: usize,
    lines: &mut Vec<Line<'static>>,
) {
    let indent = "  ".repeat(depth);
    let mut tags = Vec::new();
    if node.is_shared() {
        tags.push("[shared]");
    }
    match node.kind() {
        DependencyNodeKind::Production => {}
        DependencyNodeKind::ExternalInput => tags.push("[external]"),
        DependencyNodeKind::FuelInput => tags.push("[fuel]"),
    }
    let tag_text = if tags.is_empty() {
        String::new()
    } else {
        format!(" {}", tags.join(" "))
    };
    let machine_text = match (node.recipe(), node.machine()) {
        (Some(recipe), Some(machine)) => format!(
            " via {} on {}",
            recipe_label(catalog, recipe),
            machine_label(catalog, machine)
        ),
        _ => String::new(),
    };
    lines.push(Line::from(format!(
        "{indent}- {} {}{tag_text}{machine_text}",
        commodity_label(catalog, node.commodity()),
        format_rate(node.required_rate(), RateUnit::Second),
    )));
    for child in node.children() {
        push_dependency_node_lines(catalog, child, depth + 1, lines);
    }
}

fn render_details_pane(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let lines = selected_step_detail_lines(app);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(section_block(
                "Selected Step",
                app.focus() == FocusTarget::StepConfiguration,
            )),
        area,
    );
}

fn selected_step_detail_lines(app: &App) -> Vec<Line<'static>> {
    let catalog = active_catalog(app);
    let Some(calculation) = app.calculation() else {
        if let Some(error) = app.calculation_error() {
            return vec![
                Line::from("Calculation error"),
                Line::from(error.to_string()),
            ];
        }
        return vec![Line::from("No selected step")];
    };
    if let Some(step) = calculation
        .production_steps()
        .get(app.selected_result_index())
    {
        let mut lines = vec![
            Line::from("Selected Step"),
            Line::from(format!(
                "Product: {}",
                commodity_label(catalog, step.planning_product())
            )),
            Line::from(format!(
                "Required: {}",
                format_rate(step.required_output_rate(), calculation.display_rate_unit())
            )),
            Line::from(format!("Recipe: {}", recipe_label(catalog, step.recipe()))),
            Line::from(format!(
                "Machine: {}",
                machine_label(catalog, step.machine())
            )),
            Line::from(format!(
                "Machines: {} fractional / {} installed",
                format_quantity(step.fractional_machine_count().get()),
                step.installed_machine_count()
            )),
            Line::from(format!(
                "Modules: {}",
                module_list_label(catalog, step.modules())
            )),
            Line::from(format!("Energy: {}", step_energy_summary(step.energy()))),
            Line::from(""),
            Line::from("Ingredients"),
        ];
        push_rate_lines(
            catalog,
            step.ingredients(),
            calculation.display_rate_unit(),
            &mut lines,
        );
        lines.push(Line::from(""));
        lines.push(Line::from("Products"));
        push_rate_lines(
            catalog,
            step.products(),
            calculation.display_rate_unit(),
            &mut lines,
        );

        let belt = calculation
            .belt_equivalents()
            .iter()
            .find(|equivalent| equivalent.commodity() == step.planning_product())
            .map_or_else(
                || "Belt: none".to_owned(),
                |equivalent| {
                    format!(
                        "Belt: {} exact / {} installed",
                        format_quantity(equivalent.exact_belts().get()),
                        equivalent.installed_belts()
                    )
                },
            );
        lines.push(Line::from(""));
        lines.push(Line::from(belt));
        return lines;
    }

    let extraction_index = app
        .selected_result_index()
        .checked_sub(calculation.production_steps().len());
    let Some(step) = extraction_index.and_then(|index| calculation.extraction_steps().get(index))
    else {
        return vec![Line::from("No selected step")];
    };
    let mut lines = vec![
        Line::from("Selected Step"),
        Line::from(format!(
            "Product: {}",
            commodity_label(catalog, step.planning_product())
        )),
        Line::from(format!(
            "Required: {}",
            format_rate(step.required_output_rate(), calculation.display_rate_unit())
        )),
        Line::from(format!("Source: {}", source_label(catalog, step.source()))),
        Line::from(format!(
            "Extraction rate: {}",
            format_rate(step.extraction_rate(), calculation.display_rate_unit())
        )),
        Line::from(format!(
            "Miner: {}",
            step.mining_machine().map_or_else(
                || "none".to_owned(),
                |miner| mining_machine_label(catalog, miner)
            )
        )),
        Line::from(format!(
            "Machines: {}",
            step.fractional_machine_count().map_or_else(
                || "none".to_owned(),
                |fractional| format!(
                    "{} fractional / {} installed",
                    format_quantity(fractional.get()),
                    step.installed_machine_count()
                        .expect("mining machine count should have installed count")
                )
            )
        )),
        Line::from(format!(
            "Modules: {}",
            module_list_label(catalog, step.modules())
        )),
        Line::from(format!(
            "Energy: {}",
            step.energy()
                .map_or_else(|| "none".to_owned(), step_energy_summary)
        )),
        Line::from(""),
        Line::from("Required fluids"),
    ];
    push_rate_lines(
        catalog,
        step.required_fluids(),
        calculation.display_rate_unit(),
        &mut lines,
    );
    lines.push(Line::from(""));
    lines.push(Line::from("Products"));
    push_rate_lines(
        catalog,
        step.products(),
        calculation.display_rate_unit(),
        &mut lines,
    );
    lines
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

fn render_overlay(app: &App, overlay: &Overlay, frame: &mut Frame<'_>, area: Rect) {
    let overlay_area = centered_rect(area, 74, 14);
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
        Overlay::TextPrompt(kind) => text_prompt_lines(app, *kind),
        Overlay::Help => vec![
            Line::from("Help"),
            Line::from("j/k or arrows move selection"),
            Line::from("Tab changes focus"),
            Line::from("t switches table and dependency tree"),
            Line::from("r recipe | m machine | u modules | f fuel | b belt"),
            Line::from("x toggles external input"),
            Line::from("Esc close"),
        ],
        Overlay::Diagnostics => diagnostics_lines(app),
        Overlay::Selection(kind) => selection_lines(app, kind),
    };
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(overlay_title(overlay)),
        ),
        overlay_area,
    );
}

fn text_prompt_lines(app: &App, kind: TextPromptKind) -> Vec<Line<'static>> {
    let title = match kind {
        TextPromptKind::PlanName => "Plan name",
        TextPromptKind::TargetRate => "Target rate per second",
    };
    let mut lines = vec![
        Line::from(title),
        Line::from(""),
        Line::from(app.prompt_input().to_owned()),
        Line::from(""),
        Line::from("Enter confirm | Esc cancel"),
    ];
    if let Some(status) = app.status_message() {
        lines.push(Line::from(status.to_owned()));
    }
    lines
}

fn diagnostics_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Diagnostics")];
    if let Some(error) = app.calculation_error() {
        lines.push(Line::from(format!("Calculation error: {error}")));
    }
    if let Some(profile) = app.active_profile() {
        for diagnostic in profile.diagnostics() {
            let severity = match diagnostic.severity {
                DiagnosticSeverity::Warning => "warning",
                DiagnosticSeverity::Error => "error",
            };
            lines.push(Line::from(format!(
                "{severity}: {} {}",
                diagnostic.path, diagnostic.message
            )));
        }
    }
    if lines.len() == 1 {
        lines.push(Line::from("No diagnostics"));
    }
    lines.push(Line::from("Esc close"));
    lines
}

fn selection_lines(app: &App, kind: &SelectionKind) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Selection: {}", selection_kind_title(kind))),
        Line::from(format!("Query: {}", app.selection_query())),
        Line::from(""),
    ];
    let options = selection_option_labels(app, kind);
    if options.is_empty() {
        lines.push(Line::from("No matches"));
    } else {
        for (index, option) in options.into_iter().take(9).enumerate() {
            let marker = if index == app.selector_index() {
                ">"
            } else {
                " "
            };
            lines.push(Line::from(format!("{marker} {option}")));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Enter confirm | Esc close"));
    lines
}

fn selection_option_labels(app: &App, kind: &SelectionKind) -> Vec<String> {
    let Some(catalog) = active_catalog(app) else {
        return Vec::new();
    };
    let query = app.selection_query().to_lowercase();
    match kind {
        SelectionKind::Commodity => catalog
            .commodities()
            .filter(|commodity| {
                matches_query(&query, commodity.id().as_str(), commodity.localized_name())
            })
            .map(|commodity| commodity_label(Some(catalog), commodity.id()))
            .collect(),
        SelectionKind::Source { commodity } => catalog
            .sources_for_product(commodity)
            .iter()
            .filter(|source| source_is_selectable(catalog, source))
            .map(|source| source_label(Some(catalog), source))
            .filter(|label| query.is_empty() || label.to_lowercase().contains(&query))
            .collect(),
        SelectionKind::Recipe { commodity } => catalog
            .recipes_for_product(commodity)
            .iter()
            .filter_map(|recipe_id| catalog.recipe(recipe_id))
            .filter(|recipe| {
                recipe.supported()
                    && matches_query(&query, recipe.id().as_str(), recipe.localized_name())
            })
            .map(|recipe| recipe_label(Some(catalog), recipe.id()))
            .collect(),
        SelectionKind::Machine { recipe } => {
            catalog.recipe(recipe).map_or_else(Vec::new, |recipe| {
                catalog
                    .machines_for_category(recipe.category())
                    .iter()
                    .filter_map(|machine_id| catalog.machine(machine_id))
                    .filter(|machine| {
                        matches_query(&query, machine.id().as_str(), machine.localized_name())
                    })
                    .map(|machine| machine_label(Some(catalog), machine.id()))
                    .collect()
            })
        }
        SelectionKind::Miner { commodity } => selected_resource_source_for(app, commodity)
            .and_then(|source| catalog.resource_source(&source))
            .map_or_else(Vec::new, |resource| {
                catalog
                    .mining_machines_for_resource_category(resource.category())
                    .iter()
                    .filter_map(|miner_id| catalog.mining_machine(miner_id))
                    .filter(|miner| {
                        matches_query(&query, miner.id().as_str(), miner.localized_name())
                    })
                    .map(|miner| mining_machine_label(Some(catalog), miner.id()))
                    .collect()
            }),
        SelectionKind::Modules { .. } => catalog
            .modules()
            .filter(|module| {
                module.is_selectable()
                    && matches_query(&query, module.id().as_str(), module.localized_name())
            })
            .map(|module| module_label(Some(catalog), module.id()))
            .collect(),
        SelectionKind::Fuel { .. } => catalog
            .fuels()
            .filter(|fuel| matches_query(&query, fuel.id().as_str(), fuel.localized_name()))
            .map(|fuel| fuel_label(Some(catalog), fuel.id()))
            .collect(),
        SelectionKind::Belt => catalog
            .belts()
            .filter(|belt| matches_query(&query, belt.id().as_str(), belt.localized_name()))
            .map(|belt| belt_label(Some(catalog), belt.id()))
            .collect(),
    }
}

fn selected_resource_source_for(app: &App, commodity: &CommodityId) -> Option<ResourceSourceId> {
    app.calculation()
        .and_then(|calculation| {
            calculation
                .extraction_steps()
                .iter()
                .find(|step| step.planning_product() == commodity)
                .and_then(|step| match step.source() {
                    ProductionSource::Resource(resource) => Some(resource.clone()),
                    ProductionSource::Recipe(_)
                    | ProductionSource::Fluid(_)
                    | ProductionSource::RocketLaunch(_) => None,
                })
        })
        .or_else(|| {
            app.plan()
                .and_then(|document| document.plan().source_choice(commodity))
                .and_then(|source| match source {
                    ProductionSource::Resource(resource) => Some(resource.clone()),
                    ProductionSource::Recipe(_)
                    | ProductionSource::Fluid(_)
                    | ProductionSource::RocketLaunch(_) => None,
                })
        })
}

fn overlay_title(overlay: &Overlay) -> &'static str {
    match overlay {
        Overlay::Selection(_) => "Selection",
        Overlay::Diagnostics => "Diagnostics",
        Overlay::Help => "Help",
        Overlay::TextPrompt(kind) => match kind {
            TextPromptKind::PlanName => "Plan name",
            TextPromptKind::TargetRate => "Target rate",
        },
        Overlay::ConfirmExit
        | Overlay::ConfirmProfileReplace { .. }
        | Overlay::ConfirmProfileDelete { .. } => "Confirm",
    }
}

fn active_catalog(app: &App) -> Option<&Catalog> {
    app.active_profile()
        .map(crate::persistence::DatasetProfile::catalog)
}

fn push_rate_lines(
    catalog: Option<&Catalog>,
    rates: &[CommodityRate],
    unit: RateUnit,
    lines: &mut Vec<Line<'static>>,
) {
    if rates.is_empty() {
        lines.push(Line::from(" none"));
        return;
    }
    for rate in rates {
        lines.push(Line::from(format!(
            " {} {}",
            commodity_label(catalog, rate.commodity()),
            format_rate(rate.rate(), unit)
        )));
    }
}

fn extraction_machine_count_summary(step: &ExtractionStep) -> String {
    step.fractional_machine_count().map_or_else(
        || format_quantity(step.extraction_rate().get()),
        |fractional| {
            format!(
                "{}/{}",
                format_quantity(fractional.get()),
                step.installed_machine_count()
                    .expect("mining machine count should have installed count")
            )
        },
    )
}

fn commodity_label(catalog: Option<&Catalog>, id: &CommodityId) -> String {
    catalog
        .and_then(|catalog| catalog.commodity(id))
        .map_or_else(
            || id.to_string(),
            |commodity| label_with_id(id.as_str(), commodity.localized_name()),
        )
}

fn recipe_label(catalog: Option<&Catalog>, id: &RecipeId) -> String {
    catalog.and_then(|catalog| catalog.recipe(id)).map_or_else(
        || id.to_string(),
        |recipe| label_with_id(id.as_str(), recipe.localized_name()),
    )
}

fn machine_label(catalog: Option<&Catalog>, id: &MachineId) -> String {
    catalog.and_then(|catalog| catalog.machine(id)).map_or_else(
        || id.to_string(),
        |machine| label_with_id(id.as_str(), machine.localized_name()),
    )
}

fn mining_machine_label(catalog: Option<&Catalog>, id: &MiningMachineId) -> String {
    catalog
        .and_then(|catalog| catalog.mining_machine(id))
        .map_or_else(
            || id.to_string(),
            |machine| label_with_id(id.as_str(), machine.localized_name()),
        )
}

fn module_label(catalog: Option<&Catalog>, id: &ModuleId) -> String {
    catalog.and_then(|catalog| catalog.module(id)).map_or_else(
        || id.to_string(),
        |module| label_with_id(id.as_str(), module.localized_name()),
    )
}

fn fuel_label(catalog: Option<&Catalog>, id: &FuelId) -> String {
    catalog.and_then(|catalog| catalog.fuel(id)).map_or_else(
        || id.to_string(),
        |fuel| label_with_id(id.as_str(), fuel.localized_name()),
    )
}

fn belt_label(catalog: Option<&Catalog>, id: &BeltId) -> String {
    catalog.and_then(|catalog| catalog.belt(id)).map_or_else(
        || id.to_string(),
        |belt| label_with_id(id.as_str(), belt.localized_name()),
    )
}

fn source_label(catalog: Option<&Catalog>, source: &ProductionSource) -> String {
    match source {
        ProductionSource::Recipe(recipe) => recipe_label(catalog, recipe),
        ProductionSource::Resource(resource) => format!("resource: {resource}"),
        ProductionSource::Fluid(source_id) => catalog
            .and_then(|catalog| catalog.fluid_source(source_id))
            .map_or_else(
                || format!("fluid source: {source_id}"),
                |source| match source.kind() {
                    FluidSourceKind::OffshorePump => "offshore pump".to_owned(),
                    FluidSourceKind::BoilerSteam => "boiler steam".to_owned(),
                },
            ),
        ProductionSource::RocketLaunch(source_id) => catalog
            .and_then(|catalog| catalog.rocket_launch_source(source_id))
            .map_or_else(
                || format!("rocket launch: {source_id}"),
                |source| format!("rocket launch: {}", source.launched_item()),
            ),
    }
}

fn source_is_selectable(catalog: &Catalog, source: &ProductionSource) -> bool {
    match source {
        ProductionSource::Recipe(recipe) => catalog
            .recipe(recipe)
            .is_some_and(crate::catalog::Recipe::supported),
        ProductionSource::Resource(resource) => catalog.resource_source(resource).is_some(),
        ProductionSource::Fluid(fluid_source) => catalog.fluid_source(fluid_source).is_some(),
        ProductionSource::RocketLaunch(rocket_launch) => {
            catalog.rocket_launch_source(rocket_launch).is_some()
        }
    }
}

fn module_list_label(catalog: Option<&Catalog>, modules: &[ModuleId]) -> String {
    if modules.is_empty() {
        return "none".to_owned();
    }
    modules
        .iter()
        .map(|module| module_label(catalog, module))
        .collect::<Vec<_>>()
        .join(", ")
}

fn label_with_id(id: &str, localized_name: Option<&str>) -> String {
    localized_name.map_or_else(
        || id.to_owned(),
        |name| {
            if name == id {
                id.to_owned()
            } else {
                format!("{name} ({id})")
            }
        },
    )
}

fn format_rate(rate: Positive, unit: RateUnit) -> String {
    format!(
        "{}{}",
        format_quantity(unit.convert_rate(rate)),
        rate_unit_suffix(unit)
    )
}

fn rate_unit_suffix(unit: RateUnit) -> &'static str {
    match unit {
        RateUnit::Second => "/s",
        RateUnit::Minute => "/min",
        RateUnit::Hour => "/h",
    }
}

fn format_quantity(value: f64) -> String {
    if (value.round() - value).abs() < 1.0e-9 {
        format!("{value:.0}")
    } else if value.abs() >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn step_energy_summary(energy: &StepEnergy) -> String {
    match energy {
        StepEnergy::Electric(power) => format!(
            "Electric {}",
            format_power(power.fractional_process_watts().get())
        ),
        StepEnergy::Burner(fuel) => format!(
            "Fuel {} {}",
            fuel.fuel(),
            format_rate(fuel.rate_per_second(), RateUnit::Second)
        ),
    }
}

fn format_power(watts: f64) -> String {
    if watts >= 1_000_000.0 {
        format!("{}MW", format_quantity(watts / 1_000_000.0))
    } else if watts >= 1_000.0 {
        format!("{}kW", format_quantity(watts / 1_000.0))
    } else {
        format!("{}W", format_quantity(watts))
    }
}

fn selection_kind_title(kind: &SelectionKind) -> &'static str {
    match kind {
        SelectionKind::Commodity => "Commodity",
        SelectionKind::Source { .. } => "Source",
        SelectionKind::Recipe { .. } => "Recipe",
        SelectionKind::Machine { .. } => "Machine",
        SelectionKind::Miner { .. } => "Miner",
        SelectionKind::Modules { .. } => "Modules",
        SelectionKind::Fuel { .. } => "Fuel",
        SelectionKind::Belt => "Belt",
    }
}

fn matches_query(query: &str, id: &str, localized_name: Option<&str>) -> bool {
    query.is_empty()
        || id.to_lowercase().contains(query)
        || localized_name.is_some_and(|name| name.to_lowercase().contains(query))
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
