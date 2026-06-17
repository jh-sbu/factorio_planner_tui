use std::path::PathBuf;

use clap::Parser;
use thiserror::Error;

use crate::persistence::ProfileName;

#[derive(Clone, Debug, Eq, Parser, PartialEq)]
#[command(
    name = "factorio_planner_tui",
    about = "Plan Factorio factory production rates in a terminal UI"
)]
pub struct CliArgs {
    #[arg(long = "import-data", value_name = "PATH")]
    import_data: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    locale: Option<PathBuf>,
    #[arg(long, value_name = "NAME", value_parser = parse_profile_name)]
    profile: Option<ProfileName>,
    #[arg(long, value_name = "NAME", value_parser = parse_profile_name)]
    dataset: Option<ProfileName>,
    #[arg(long, value_name = "PATH")]
    plan: Option<PathBuf>,
}

impl CliArgs {
    #[must_use]
    pub fn into_startup_request(self) -> StartupRequest {
        StartupRequest {
            import_data: self.import_data,
            locale: self.locale,
            profile: self.profile,
            dataset: self.dataset,
            plan: self.plan,
        }
    }
}

fn parse_profile_name(value: &str) -> Result<ProfileName, String> {
    ProfileName::new(value).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartupRequest {
    import_data: Option<PathBuf>,
    locale: Option<PathBuf>,
    profile: Option<ProfileName>,
    dataset: Option<ProfileName>,
    plan: Option<PathBuf>,
}

impl StartupRequest {
    #[must_use]
    pub fn with_import_data(mut self, path: impl Into<PathBuf>) -> Self {
        self.import_data = Some(path.into());
        self
    }

    #[must_use]
    pub fn with_locale(mut self, path: impl Into<PathBuf>) -> Self {
        self.locale = Some(path.into());
        self
    }

    #[must_use]
    pub fn with_profile(mut self, profile: ProfileName) -> Self {
        self.profile = Some(profile);
        self
    }

    #[must_use]
    pub fn with_dataset(mut self, profile: ProfileName) -> Self {
        self.dataset = Some(profile);
        self
    }

    #[must_use]
    pub fn with_plan(mut self, path: impl Into<PathBuf>) -> Self {
        self.plan = Some(path.into());
        self
    }

    /// Resolves parsed startup options into one deterministic startup path.
    ///
    /// # Errors
    ///
    /// Returns [`StartupInputError`] for option combinations that would make
    /// startup ambiguous or impossible.
    pub fn resolve(self) -> Result<StartupMode, StartupInputError> {
        if self.locale.is_some() && self.import_data.is_none() {
            return Err(StartupInputError::LocaleRequiresImportData);
        }
        if self.profile.is_some() && self.import_data.is_none() {
            return Err(StartupInputError::ProfileRequiresImportData);
        }
        if self.plan.is_some() && self.dataset.is_some() {
            return Err(StartupInputError::PlanConflictsWithDatasetSelection);
        }
        if self.plan.is_some() && self.import_data.is_some() {
            return Err(StartupInputError::PlanConflictsWithImport);
        }
        if self.dataset.is_some() && self.import_data.is_some() {
            return Err(StartupInputError::DatasetConflictsWithImport);
        }

        if let Some(path) = self.plan {
            return Ok(StartupMode::OpenPlan { path });
        }
        if let Some(profile) = self.dataset {
            return Ok(StartupMode::OpenDataset { profile });
        }
        if let Some(data_path) = self.import_data {
            return Ok(StartupMode::ImportData {
                data_path,
                locale_path: self.locale,
                profile: self.profile,
            });
        }
        Ok(StartupMode::StartScreen)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupMode {
    StartScreen,
    ImportData {
        data_path: PathBuf,
        locale_path: Option<PathBuf>,
        profile: Option<ProfileName>,
    },
    OpenDataset {
        profile: ProfileName,
    },
    OpenPlan {
        path: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum StartupInputError {
    #[error("--locale requires --import-data")]
    LocaleRequiresImportData,
    #[error("--profile requires --import-data")]
    ProfileRequiresImportData,
    #[error("--plan cannot be combined with --dataset")]
    PlanConflictsWithDatasetSelection,
    #[error("--plan cannot be combined with --import-data")]
    PlanConflictsWithImport,
    #[error("--dataset cannot be combined with --import-data")]
    DatasetConflictsWithImport,
}
