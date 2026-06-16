pub mod app;
pub mod catalog;
pub mod cli;
pub mod import;
pub mod persistence;
pub mod planner;
pub mod tui;

use thiserror::Error;

use crate::app::{App, AppError};
use crate::cli::StartupMode;
use crate::persistence::{PlanFileStore, ProfileError, ProfileStore};
use crate::tui::TuiError;

/// Runs the interactive terminal application.
///
/// # Errors
///
/// Returns [`RunError`] when startup, logging, or terminal interaction fails.
pub fn run() -> Result<(), RunError> {
    let profiles = ProfileStore::for_current_user()?;
    let _logging_guard = tui::initialize_file_logging(profiles.root())?;
    let plans = PlanFileStore::new();
    let mut app = App::start(StartupMode::StartScreen, &profiles, &plans)?;
    tui::run_app(&mut app, &profiles, &plans)?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(transparent)]
    App(#[from] AppError),
    #[error(transparent)]
    Tui(#[from] TuiError),
}
