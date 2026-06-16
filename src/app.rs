use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::catalog::{BeltId, CommodityId, FuelId, MachineId, ModuleId, RecipeId};
use crate::cli::StartupMode;
use crate::persistence::{
    BlockedPlanDocument, DatasetProfile, PlanDocument, PlanFileError, PlanFileStore, PlanName,
    ProfileError, ProfileName, ProfileStore, ProfileSummary,
};
use crate::planner::{
    CalculationResult, FactoryPlan, PlanEditError, PlannerError, RateUnit, Target, calculate,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    Start,
    Import,
    Profiles,
    PlanningWorkspace,
    BlockedPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusTarget {
    StartMenu,
    ProfileList,
    TargetList,
    Results,
    StepConfiguration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceView {
    AggregatedTable,
    DependencyTree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    Selection(SelectionKind),
    Diagnostics,
    Help,
    ConfirmExit,
    ConfirmProfileReplace { profile: ProfileName },
    ConfirmProfileDelete { profile: ProfileName },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionKind {
    Commodity,
    Recipe { commodity: CommodityId },
    Machine { recipe: RecipeId },
    Modules { commodity: CommodityId },
    Fuel { commodity: CommodityId },
    Belt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitState {
    Running,
    WaitingForConfirmation,
    Confirmed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingImport {
    data_path: PathBuf,
    locale_path: Option<PathBuf>,
    profile: Option<ProfileName>,
}

impl PendingImport {
    #[must_use]
    pub const fn data_path(&self) -> &PathBuf {
        &self.data_path
    }

    #[must_use]
    pub const fn locale_path(&self) -> Option<&PathBuf> {
        self.locale_path.as_ref()
    }

    #[must_use]
    pub const fn profile(&self) -> Option<&ProfileName> {
        self.profile.as_ref()
    }
}

#[derive(Clone, Debug)]
pub struct App {
    screen: Screen,
    focus: FocusTarget,
    overlay: Option<Overlay>,
    profiles: Vec<ProfileSummary>,
    active_profile: Option<DatasetProfile>,
    plan: Option<PlanDocument>,
    blocked_plan: Option<BlockedPlanDocument>,
    calculation: Option<CalculationResult>,
    calculation_error: Option<PlannerError>,
    workspace_view: WorkspaceView,
    exit_state: ExitState,
    pending_import: Option<PendingImport>,
    status_message: Option<String>,
}

impl App {
    /// Builds application state from a resolved startup mode.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when startup needs a profile or plan file that
    /// cannot be read.
    pub fn start(
        mode: StartupMode,
        profiles: &ProfileStore,
        plans: &PlanFileStore,
    ) -> Result<Self, AppError> {
        let mut app = Self::new();
        app.refresh_profiles(profiles)?;

        match mode {
            StartupMode::StartScreen => {}
            StartupMode::ImportData {
                data_path,
                locale_path,
                profile,
            } => {
                app.screen = Screen::Import;
                app.focus = FocusTarget::StartMenu;
                app.pending_import = Some(PendingImport {
                    data_path,
                    locale_path,
                    profile,
                });
            }
            StartupMode::OpenDataset { profile } => {
                app.select_profile(&profile, profiles)?;
            }
            StartupMode::OpenPlan { path } => {
                app.open_plan(&path, profiles, plans)?;
            }
        }

        Ok(app)
    }

    #[must_use]
    pub fn new() -> Self {
        Self {
            screen: Screen::Start,
            focus: FocusTarget::StartMenu,
            overlay: None,
            profiles: Vec::new(),
            active_profile: None,
            plan: None,
            blocked_plan: None,
            calculation: None,
            calculation_error: None,
            workspace_view: WorkspaceView::AggregatedTable,
            exit_state: ExitState::Running,
            pending_import: None,
            status_message: None,
        }
    }

    /// Applies an application action without requiring terminal or rendering
    /// state.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the action needs missing state, invalid plan
    /// edits, or persistence operations that fail.
    pub fn dispatch(
        &mut self,
        action: Action,
        profiles: &ProfileStore,
        plans: &PlanFileStore,
    ) -> Result<(), AppError> {
        if self.dispatch_session_action(&action, profiles, plans)? {
            return Ok(());
        }
        self.dispatch_plan_action(action)
    }

    fn dispatch_session_action(
        &mut self,
        action: &Action,
        profiles: &ProfileStore,
        plans: &PlanFileStore,
    ) -> Result<bool, AppError> {
        match action {
            Action::RefreshProfiles => self.refresh_profiles(profiles)?,
            Action::SelectProfile { profile } => self.select_profile(profile, profiles)?,
            Action::OpenPlan { path } => self.open_plan(path, profiles, plans)?,
            Action::CreatePlan {
                name,
                profile,
                target,
            } => self.create_plan(name.clone(), profile, target.clone(), profiles)?,
            Action::SavePlan { path } => self.save_plan(path, plans)?,
            Action::RebindBlockedPlan { profile } => {
                self.rebind_blocked_plan(profile, profiles, plans)?;
            }
            Action::ReportImportSuccess {
                profile,
                warning_count,
            } => {
                self.screen = Screen::Import;
                self.pending_import = None;
                self.status_message = Some(format!(
                    "Imported profile {profile} with {}",
                    pluralize(*warning_count, "warning")
                ));
            }
            Action::ReportImportFailure { message } => {
                self.screen = Screen::Import;
                self.status_message = Some(format!("Import failed: {message}"));
            }
            Action::SetWorkspaceView(view) => self.workspace_view = *view,
            Action::MoveFocus(focus) => self.focus = *focus,
            Action::OpenOverlay(overlay) => self.overlay = Some(overlay.clone()),
            Action::CloseOverlay => self.overlay = None,
            Action::RequestExit => self.request_exit(),
            Action::ConfirmExit => {
                self.overlay = None;
                self.exit_state = ExitState::Confirmed;
            }
            Action::CancelExit => self.cancel_exit(),
            Action::DismissStatus => self.status_message = None,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn dispatch_plan_action(&mut self, action: Action) -> Result<(), AppError> {
        match action {
            Action::AddTarget(target) => {
                self.edit_plan(|plan| {
                    plan.add_target(target);
                })?;
                self.recalculate_current()
            }
            Action::ReplaceTarget { index, target } => {
                self.try_edit_plan(|plan| plan.replace_target(index, target).map(|_| ()))?;
                self.recalculate_current()
            }
            Action::RemoveTarget { index } => {
                self.try_edit_plan(|plan| plan.remove_target(index).map(|_| ()))?;
                self.recalculate_current()
            }
            Action::SetRecipeChoice { commodity, recipe } => {
                self.edit_plan(|plan| {
                    plan.set_recipe_choice(commodity, recipe);
                })?;
                self.recalculate_current()
            }
            Action::ClearRecipeChoice { commodity } => {
                self.edit_plan(|plan| {
                    plan.clear_recipe_choice(&commodity);
                })?;
                self.recalculate_current()
            }
            Action::SetMachineChoice { recipe, machine } => {
                self.edit_plan(|plan| {
                    plan.set_machine_choice(recipe, machine);
                })?;
                self.recalculate_current()
            }
            Action::ClearMachineChoice { recipe } => {
                self.edit_plan(|plan| {
                    plan.clear_machine_choice(&recipe);
                })?;
                self.recalculate_current()
            }
            Action::SetModules { commodity, modules } => {
                self.edit_plan(|plan| {
                    plan.set_modules(commodity, modules);
                })?;
                self.recalculate_current()
            }
            Action::ClearModules { commodity } => {
                self.edit_plan(|plan| {
                    plan.clear_modules(&commodity);
                })?;
                self.recalculate_current()
            }
            Action::SetFuelChoice { commodity, fuel } => {
                self.edit_plan(|plan| {
                    plan.set_fuel_choice(commodity, fuel);
                })?;
                self.recalculate_current()
            }
            Action::ClearFuelChoice { commodity } => {
                self.edit_plan(|plan| {
                    plan.clear_fuel_choice(&commodity);
                })?;
                self.recalculate_current()
            }
            Action::ToggleExternalInput { commodity } => {
                self.edit_plan(|plan| {
                    plan.toggle_external_input(commodity);
                })?;
                self.recalculate_current()
            }
            Action::SetSelectedBelt { belt } => {
                self.edit_plan(|plan| match belt {
                    Some(belt) => {
                        plan.set_selected_belt(belt);
                    }
                    None => {
                        plan.clear_selected_belt();
                    }
                })?;
                self.recalculate_current()
            }
            Action::SetDisplayRateUnit { unit } => {
                self.edit_plan(|plan| {
                    plan.set_display_rate_unit(unit);
                })?;
                self.recalculate_current()
            }
            _ => Ok(()),
        }
    }

    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    #[must_use]
    pub const fn focus(&self) -> FocusTarget {
        self.focus
    }

    #[must_use]
    pub const fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    #[must_use]
    pub fn profiles(&self) -> &[ProfileSummary] {
        &self.profiles
    }

    #[must_use]
    pub const fn active_profile(&self) -> Option<&DatasetProfile> {
        self.active_profile.as_ref()
    }

    #[must_use]
    pub const fn plan(&self) -> Option<&PlanDocument> {
        self.plan.as_ref()
    }

    #[must_use]
    pub const fn blocked_plan(&self) -> Option<&BlockedPlanDocument> {
        self.blocked_plan.as_ref()
    }

    #[must_use]
    pub const fn calculation(&self) -> Option<&CalculationResult> {
        self.calculation.as_ref()
    }

    #[must_use]
    pub const fn calculation_error(&self) -> Option<&PlannerError> {
        self.calculation_error.as_ref()
    }

    #[must_use]
    pub const fn workspace_view(&self) -> WorkspaceView {
        self.workspace_view
    }

    #[must_use]
    pub const fn exit_state(&self) -> ExitState {
        self.exit_state
    }

    #[must_use]
    pub const fn pending_import(&self) -> Option<&PendingImport> {
        self.pending_import.as_ref()
    }

    #[must_use]
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    fn refresh_profiles(&mut self, profiles: &ProfileStore) -> Result<(), AppError> {
        self.profiles = profiles.list()?;
        if let Some(active) = profiles.active_profile_name()? {
            self.active_profile = Some(profiles.open(&active)?);
        }
        Ok(())
    }

    fn select_profile(
        &mut self,
        profile: &ProfileName,
        profiles: &ProfileStore,
    ) -> Result<(), AppError> {
        if self.plan.as_ref().is_some_and(PlanDocument::is_dirty) {
            return Err(AppError::DirtyPlanBlocksProfileSelection);
        }
        let opened = profiles.open(profile)?;
        profiles.select(profile)?;
        self.active_profile = Some(opened);
        self.plan = None;
        self.blocked_plan = None;
        self.calculation = None;
        self.calculation_error = None;
        self.screen = Screen::Start;
        self.focus = FocusTarget::StartMenu;
        self.status_message = Some(format!("selected dataset profile {profile}"));
        self.refresh_profiles(profiles)?;
        Ok(())
    }

    fn open_plan(
        &mut self,
        path: &Path,
        profiles: &ProfileStore,
        plans: &PlanFileStore,
    ) -> Result<(), AppError> {
        if self.plan.as_ref().is_some_and(PlanDocument::is_dirty) {
            return Err(AppError::DirtyPlanBlocksOpenPlan);
        }

        match plans.open(path, profiles)? {
            crate::persistence::PlanOpenResult::Ready(document) => {
                let profile = profiles.open(document.dataset_profile())?;
                self.active_profile = Some(profile);
                self.plan = Some(document);
                self.blocked_plan = None;
                self.screen = Screen::PlanningWorkspace;
                self.focus = FocusTarget::TargetList;
                self.workspace_view = WorkspaceView::AggregatedTable;
                self.recalculate_current()?;
            }
            crate::persistence::PlanOpenResult::Blocked(blocked) => {
                self.plan = None;
                self.blocked_plan = Some(blocked);
                self.calculation = None;
                self.calculation_error = None;
                self.screen = Screen::BlockedPlan;
                self.focus = FocusTarget::StartMenu;
            }
        }
        Ok(())
    }

    fn create_plan(
        &mut self,
        name: PlanName,
        profile: &ProfileName,
        target: Target,
        profiles: &ProfileStore,
    ) -> Result<(), AppError> {
        if self.plan.as_ref().is_some_and(PlanDocument::is_dirty) {
            return Err(AppError::DirtyPlanBlocksCreatePlan);
        }
        let profile = profiles.open(profile)?;
        let document = PlanDocument::new(
            name,
            profile.name().clone(),
            profile.fingerprint().clone(),
            FactoryPlan::new(target),
        );
        self.active_profile = Some(profile);
        self.plan = Some(document);
        self.blocked_plan = None;
        self.screen = Screen::PlanningWorkspace;
        self.focus = FocusTarget::TargetList;
        self.workspace_view = WorkspaceView::AggregatedTable;
        self.recalculate_current()
    }

    fn save_plan(&mut self, path: &Path, plans: &PlanFileStore) -> Result<(), AppError> {
        let document = self.plan.as_mut().ok_or(AppError::NoOpenPlan)?;
        plans.save(path, document)?;
        self.status_message = Some(format!("saved plan to {}", path.display()));
        Ok(())
    }

    fn rebind_blocked_plan(
        &mut self,
        profile: &ProfileName,
        profiles: &ProfileStore,
        plans: &PlanFileStore,
    ) -> Result<(), AppError> {
        let blocked = self.blocked_plan.clone().ok_or(AppError::NoBlockedPlan)?;
        let profile = profiles.open(profile)?;
        let document = plans.rebind(blocked, &profile)?;
        self.active_profile = Some(profile);
        self.plan = Some(document);
        self.blocked_plan = None;
        self.screen = Screen::PlanningWorkspace;
        self.focus = FocusTarget::TargetList;
        self.workspace_view = WorkspaceView::AggregatedTable;
        self.recalculate_current()
    }

    fn request_exit(&mut self) {
        if self.plan.as_ref().is_some_and(PlanDocument::is_dirty) {
            self.overlay = Some(Overlay::ConfirmExit);
            self.exit_state = ExitState::WaitingForConfirmation;
        } else {
            self.overlay = None;
            self.exit_state = ExitState::Confirmed;
        }
    }

    fn cancel_exit(&mut self) {
        if self.exit_state == ExitState::WaitingForConfirmation {
            self.overlay = None;
            self.exit_state = ExitState::Running;
        }
    }

    fn edit_plan(&mut self, edit: impl FnOnce(&mut FactoryPlan)) -> Result<(), AppError> {
        let document = self.plan.as_mut().ok_or(AppError::NoOpenPlan)?;
        document.edit_plan(edit);
        Ok(())
    }

    fn try_edit_plan(
        &mut self,
        edit: impl FnOnce(&mut FactoryPlan) -> Result<(), PlanEditError>,
    ) -> Result<(), AppError> {
        let document = self.plan.as_mut().ok_or(AppError::NoOpenPlan)?;
        document.try_edit_plan(edit)?;
        Ok(())
    }

    fn recalculate_current(&mut self) -> Result<(), AppError> {
        let Some(document) = self.plan.as_ref() else {
            self.calculation = None;
            self.calculation_error = None;
            return Ok(());
        };
        let profile = self
            .active_profile
            .as_ref()
            .ok_or(AppError::NoActiveProfile)?;
        match calculate(profile.catalog(), document.plan()) {
            Ok(result) => {
                self.calculation = Some(result);
                self.calculation_error = None;
            }
            Err(error) => {
                self.calculation = None;
                self.calculation_error = Some(error);
            }
        }
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    RefreshProfiles,
    SelectProfile {
        profile: ProfileName,
    },
    OpenPlan {
        path: PathBuf,
    },
    CreatePlan {
        name: PlanName,
        profile: ProfileName,
        target: Target,
    },
    SavePlan {
        path: PathBuf,
    },
    RebindBlockedPlan {
        profile: ProfileName,
    },
    ReportImportSuccess {
        profile: ProfileName,
        warning_count: usize,
    },
    ReportImportFailure {
        message: String,
    },
    AddTarget(Target),
    ReplaceTarget {
        index: usize,
        target: Target,
    },
    RemoveTarget {
        index: usize,
    },
    SetRecipeChoice {
        commodity: CommodityId,
        recipe: RecipeId,
    },
    ClearRecipeChoice {
        commodity: CommodityId,
    },
    SetMachineChoice {
        recipe: RecipeId,
        machine: MachineId,
    },
    ClearMachineChoice {
        recipe: RecipeId,
    },
    SetModules {
        commodity: CommodityId,
        modules: Vec<ModuleId>,
    },
    ClearModules {
        commodity: CommodityId,
    },
    SetFuelChoice {
        commodity: CommodityId,
        fuel: FuelId,
    },
    ClearFuelChoice {
        commodity: CommodityId,
    },
    ToggleExternalInput {
        commodity: CommodityId,
    },
    SetSelectedBelt {
        belt: Option<BeltId>,
    },
    SetDisplayRateUnit {
        unit: RateUnit,
    },
    SetWorkspaceView(WorkspaceView),
    MoveFocus(FocusTarget),
    OpenOverlay(Overlay),
    CloseOverlay,
    RequestExit,
    ConfirmExit,
    CancelExit,
    DismissStatus,
}

fn pluralize(count: usize, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("no factory plan is open")]
    NoOpenPlan,
    #[error("no blocked factory plan is available")]
    NoBlockedPlan,
    #[error("no active dataset profile is available")]
    NoActiveProfile,
    #[error("save or discard the dirty plan before selecting another profile")]
    DirtyPlanBlocksProfileSelection,
    #[error("save or discard the dirty plan before opening another plan")]
    DirtyPlanBlocksOpenPlan,
    #[error("save or discard the dirty plan before creating another plan")]
    DirtyPlanBlocksCreatePlan,
    #[error(transparent)]
    PlanEdit(#[from] PlanEditError),
    #[error(transparent)]
    PlanFile(#[from] PlanFileError),
    #[error(transparent)]
    Profile(#[from] ProfileError),
}
