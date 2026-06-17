pub mod app;
pub mod catalog;
pub mod cli;
pub mod import;
pub mod persistence;
pub mod planner;
pub mod tui;

use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::app::{Action, App, AppError};
use crate::cli::{CliArgs, StartupInputError, StartupMode};
use crate::import::LocalePrototypeKind;
use crate::persistence::{PlanFileStore, ProfileError, ProfileImportRequest, ProfileStore};
use crate::tui::TuiError;

/// Runs the interactive terminal application.
///
/// # Errors
///
/// Returns [`RunError`] when startup, logging, or terminal interaction fails.
pub fn run() -> Result<(), RunError> {
    let mode = CliArgs::parse().into_startup_request().resolve()?;
    let profiles = ProfileStore::for_current_user()?;
    let _logging_guard = tui::initialize_file_logging(profiles.root())?;
    let plans = PlanFileStore::new();
    run_with_startup_mode(mode, &profiles, &plans, tui::run_app)
}

/// Runs a resolved startup mode with an injectable terminal launcher.
///
/// # Errors
///
/// Returns [`RunError`] when pre-TUI import, application startup, or terminal
/// execution fails.
pub fn run_with_startup_mode(
    mode: StartupMode,
    profiles: &ProfileStore,
    plans: &PlanFileStore,
    launch_tui: impl FnOnce(&mut App, &ProfileStore, &PlanFileStore) -> Result<(), TuiError>,
) -> Result<(), RunError> {
    let mut app = app_for_startup_mode(mode, profiles, plans)?;
    launch_tui(&mut app, profiles, plans)?;
    Ok(())
}

fn app_for_startup_mode(
    mode: StartupMode,
    profiles: &ProfileStore,
    plans: &PlanFileStore,
) -> Result<App, RunError> {
    match mode {
        StartupMode::ImportData {
            data_path,
            locale_path,
            profile: Some(profile),
        } => {
            let request =
                profile_import_request(profile.clone(), &data_path, locale_path.as_deref())?;
            let imported = profiles.create(&request)?;
            profiles.select(&profile)?;
            let mut app = App::start(StartupMode::StartScreen, profiles, plans)?;
            app.dispatch(
                Action::ReportImportSuccess {
                    profile,
                    warning_count: imported.warning_count(),
                },
                profiles,
                plans,
            )?;
            Ok(app)
        }
        other => Ok(App::start(other, profiles, plans)?),
    }
}

fn profile_import_request(
    profile: crate::persistence::ProfileName,
    data_path: &Path,
    locale_path: Option<&Path>,
) -> Result<ProfileImportRequest, RunError> {
    let mut request = ProfileImportRequest::new(profile, data_path.to_path_buf());
    if let Some(locale_path) = locale_path {
        request = add_locale_directory(request, locale_path)?;
    }
    Ok(request)
}

fn add_locale_directory(
    mut request: ProfileImportRequest,
    locale_path: &Path,
) -> Result<ProfileImportRequest, RunError> {
    if !locale_path.is_dir() {
        return Err(RunError::LocalePathNotDirectory {
            path: locale_path.to_path_buf(),
        });
    }

    let mut found = false;
    for (file_name, kind) in [
        ("item-locale.json", LocalePrototypeKind::Item),
        ("fluid-locale.json", LocalePrototypeKind::Fluid),
        ("recipe-locale.json", LocalePrototypeKind::Recipe),
        ("entity-locale.json", LocalePrototypeKind::Entity),
    ] {
        let path = locale_path.join(file_name);
        if path.is_file() {
            request = request.with_locale_path(kind, path);
            found = true;
        }
    }

    if !found {
        return Err(RunError::EmptyLocaleDirectory {
            path: locale_path.to_path_buf(),
        });
    }

    Ok(request)
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    StartupInput(#[from] StartupInputError),
    #[error("locale path {path:?} must be a directory containing prototype locale JSON files")]
    LocalePathNotDirectory { path: PathBuf },
    #[error("locale directory {path:?} does not contain recognized prototype locale JSON files")]
    EmptyLocaleDirectory { path: PathBuf },
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(transparent)]
    App(#[from] AppError),
    #[error(transparent)]
    Tui(#[from] TuiError),
}
