use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{
    Belt, BeltId, Catalog, CatalogParts, Commodity, CommodityId, DatasetFingerprint, Finite,
    FluidId, FluidProperties, FluidSource, FluidSourceId, FluidSourceKind, Fuel, FuelCategory,
    FuelId, Ingredient, ItemId, Machine, MachineEnergySource, MachineId, MiningMachine,
    MiningMachineId, Module, ModuleCategory, ModuleEffect, ModuleId, NonNegative, Positive,
    Product, ProductionSource, Recipe, RecipeCategory, RecipeId, ResourceCategory, ResourceSource,
    ResourceSourceId, RocketLaunchSource, RocketLaunchSourceId, UnsupportedEnergySource,
};
use crate::import::{
    DiagnosticSeverity, ImportDiagnostic, ImportError, LocaleError, LocalePrototypeKind,
    PrototypeDisposition, parse_data_raw, parse_data_raw_with_locale, parse_prototype_locale,
};
use crate::planner::{FactoryPlan, RateUnit, Target};

pub const PROFILE_INDEX_SCHEMA_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 4;
pub const IMPORTER_SCHEMA_VERSION: u32 = 4;
pub const PLAN_SCHEMA_VERSION: u32 = 2;
pub const PLAN_FILE_SUFFIX: &str = ".fptplan.json";

const INDEX_FILE_NAME: &str = "profiles.json";
const CATALOG_DIRECTORY_NAME: &str = "catalogs";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProfileName(String);

impl ProfileName {
    /// Creates a case-sensitive profile name after trimming outer whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileNameError`] for empty names or names containing control
    /// characters.
    pub fn new(value: impl Into<String>) -> Result<Self, ProfileNameError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(ProfileNameError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(ProfileNameError::ControlCharacter);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProfileNameError {
    #[error("profile name must not be empty")]
    Empty,
    #[error("profile name must not contain control characters")]
    ControlCharacter,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlanName(String);

impl PlanName {
    /// Creates a case-sensitive plan name after trimming outer whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`PlanNameError`] for empty names or names containing control
    /// characters.
    pub fn new(value: impl Into<String>) -> Result<Self, PlanNameError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(PlanNameError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(PlanNameError::ControlCharacter);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlanName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PlanNameError {
    #[error("plan name must not be empty")]
    Empty,
    #[error("plan name must not contain control characters")]
    ControlCharacter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMetadata {
    path: PathBuf,
    fingerprint: String,
}

impl SourceMetadata {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleSourceMetadata {
    kind: LocalePrototypeKind,
    source: SourceMetadata,
}

impl LocaleSourceMetadata {
    #[must_use]
    pub const fn kind(&self) -> LocalePrototypeKind {
        self.kind
    }

    #[must_use]
    pub const fn source(&self) -> &SourceMetadata {
        &self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMetadata {
    imported_at_unix_seconds: u64,
    importer_schema_version: u32,
    data_source: SourceMetadata,
    locale_sources: Vec<LocaleSourceMetadata>,
}

impl ImportMetadata {
    #[must_use]
    pub const fn imported_at_unix_seconds(&self) -> u64 {
        self.imported_at_unix_seconds
    }

    #[must_use]
    pub const fn importer_schema_version(&self) -> u32 {
        self.importer_schema_version
    }

    #[must_use]
    pub const fn data_source(&self) -> &SourceMetadata {
        &self.data_source
    }

    #[must_use]
    pub fn locale_sources(&self) -> &[LocaleSourceMetadata] {
        &self.locale_sources
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DatasetProfile {
    name: ProfileName,
    fingerprint: DatasetFingerprint,
    catalog: Catalog,
    metadata: ImportMetadata,
    diagnostics: Vec<ImportDiagnostic>,
}

impl DatasetProfile {
    #[must_use]
    pub const fn name(&self) -> &ProfileName {
        &self.name
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &DatasetFingerprint {
        &self.fingerprint
    }

    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    #[must_use]
    pub const fn metadata(&self) -> &ImportMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[ImportDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn warning_count(&self) -> usize {
        warning_count(&self.diagnostics)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSummary {
    name: ProfileName,
    fingerprint: DatasetFingerprint,
    metadata: ImportMetadata,
    warning_count: usize,
}

impl ProfileSummary {
    #[must_use]
    pub const fn name(&self) -> &ProfileName {
        &self.name
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &DatasetFingerprint {
        &self.fingerprint
    }

    #[must_use]
    pub const fn metadata(&self) -> &ImportMetadata {
        &self.metadata
    }

    #[must_use]
    pub const fn warning_count(&self) -> usize {
        self.warning_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileImportRequest {
    name: ProfileName,
    data_path: PathBuf,
    locale_paths: BTreeMap<LocalePrototypeKind, PathBuf>,
}

impl ProfileImportRequest {
    #[must_use]
    pub fn new(name: ProfileName, data_path: impl Into<PathBuf>) -> Self {
        Self {
            name,
            data_path: data_path.into(),
            locale_paths: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_locale_path(mut self, kind: LocalePrototypeKind, path: impl Into<PathBuf>) -> Self {
        self.locale_paths.insert(kind, path.into());
        self
    }

    #[must_use]
    pub const fn name(&self) -> &ProfileName {
        &self.name
    }

    #[must_use]
    pub fn data_path(&self) -> &Path {
        &self.data_path
    }

    #[must_use]
    pub const fn locale_paths(&self) -> &BTreeMap<LocalePrototypeKind, PathBuf> {
        &self.locale_paths
    }
}

#[derive(Clone, Debug)]
pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves the platform application-data directory.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::ApplicationDataDirectoryUnavailable`] when the
    /// current platform does not expose an application-data directory.
    pub fn for_current_user() -> Result<Self, ProfileError> {
        let directories = ProjectDirs::from("com", "FactorioPlanner", "factorio-planner-tui")
            .ok_or(ProfileError::ApplicationDataDirectoryUnavailable)?;
        Ok(Self::new(directories.data_dir()))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE_NAME)
    }

    #[must_use]
    pub fn catalogs_dir(&self) -> PathBuf {
        self.root.join(CATALOG_DIRECTORY_NAME)
    }

    #[must_use]
    pub fn catalog_path(&self, fingerprint: &DatasetFingerprint) -> PathBuf {
        self.catalogs_dir()
            .join(format!("{}.json", fingerprint.as_str()))
    }

    /// Imports and creates a profile.
    ///
    /// # Errors
    ///
    /// Returns a structured error for duplicate names, invalid source data, or
    /// filesystem failures.
    pub fn create(&self, request: &ProfileImportRequest) -> Result<DatasetProfile, ProfileError> {
        let mut index = self.load_index()?;
        if index.profiles.contains_key(request.name.as_str()) {
            return Err(ProfileError::ProfileAlreadyExists {
                name: request.name.clone(),
            });
        }

        let imported = import_request(request)?;
        self.write_catalog_if_absent(&imported.fingerprint, &imported.catalog)?;

        let stored = StoredProfile::from_imported(&imported);
        index
            .profiles
            .insert(request.name.as_str().to_owned(), stored);
        if index.active_profile.is_none() {
            index.active_profile = Some(request.name.as_str().to_owned());
        }
        self.write_index(&index)?;

        Ok(imported.into_profile(request.name.clone()))
    }

    /// Imports new data and atomically replaces an existing profile mapping.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the profile does not exist, import
    /// fails, or the new catalog and index cannot be persisted.
    pub fn replace(&self, request: &ProfileImportRequest) -> Result<DatasetProfile, ProfileError> {
        let mut index = self.load_index()?;
        if !index.profiles.contains_key(request.name.as_str()) {
            return Err(ProfileError::ProfileNotFound {
                name: request.name.clone(),
            });
        }

        let imported = import_request(request)?;
        self.write_catalog_if_absent(&imported.fingerprint, &imported.catalog)?;
        index.profiles.insert(
            request.name.as_str().to_owned(),
            StoredProfile::from_imported(&imported),
        );
        self.write_index(&index)?;

        Ok(imported.into_profile(request.name.clone()))
    }

    /// Lists profiles in case-sensitive lexical name order.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile index cannot be read or validated.
    pub fn list(&self) -> Result<Vec<ProfileSummary>, ProfileError> {
        let index = self.load_index()?;
        index
            .profiles
            .into_iter()
            .map(|(name, profile)| profile.into_summary(name))
            .collect()
    }

    /// Opens a cached profile without reading its original source dumps.
    ///
    /// # Errors
    ///
    /// Returns an error for missing profiles, corrupt files, unsupported
    /// versions, fingerprint mismatches, or invalid normalized catalog data.
    pub fn open(&self, name: &ProfileName) -> Result<DatasetProfile, ProfileError> {
        let index = self.load_index()?;
        let stored = index
            .profiles
            .get(name.as_str())
            .ok_or_else(|| ProfileError::ProfileNotFound { name: name.clone() })?;
        let fingerprint = dataset_fingerprint(&stored.dataset_fingerprint)?;
        let catalog = self.read_catalog(&fingerprint)?;
        stored.to_profile(name.clone(), fingerprint, catalog)
    }

    /// Finds the first profile in lexical name order with an exact dataset
    /// fingerprint match.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile index or matching cached catalog
    /// cannot be read.
    pub fn find_by_fingerprint(
        &self,
        fingerprint: &DatasetFingerprint,
    ) -> Result<Option<DatasetProfile>, ProfileError> {
        for summary in self.list()? {
            if summary.fingerprint() == fingerprint {
                return self.open(summary.name()).map(Some);
            }
        }
        Ok(None)
    }

    /// Selects the active profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is absent or the index cannot be
    /// persisted.
    pub fn select(&self, name: &ProfileName) -> Result<(), ProfileError> {
        let mut index = self.load_index()?;
        if !index.profiles.contains_key(name.as_str()) {
            return Err(ProfileError::ProfileNotFound { name: name.clone() });
        }
        index.active_profile = Some(name.as_str().to_owned());
        self.write_index(&index)
    }

    /// Returns the active profile name.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is unreadable or internally
    /// inconsistent.
    pub fn active_profile_name(&self) -> Result<Option<ProfileName>, ProfileError> {
        let index = self.load_index()?;
        index
            .active_profile
            .map(|name| {
                if !index.profiles.contains_key(&name) {
                    return Err(ProfileError::InvalidIndex {
                        message: format!("active profile {name} is not present in the index"),
                    });
                }
                ProfileName::new(name).map_err(|error| ProfileError::InvalidIndex {
                    message: error.to_string(),
                })
            })
            .transpose()
    }

    /// Removes a profile mapping. Cached catalogs are retained so profiles
    /// sharing a fingerprint and interrupted replacements remain recoverable.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is absent or the updated index cannot
    /// be persisted.
    pub fn delete(&self, name: &ProfileName) -> Result<(), ProfileError> {
        let mut index = self.load_index()?;
        if index.profiles.remove(name.as_str()).is_none() {
            return Err(ProfileError::ProfileNotFound { name: name.clone() });
        }
        if index.active_profile.as_deref() == Some(name.as_str()) {
            index.active_profile = None;
        }
        self.write_index(&index)
    }

    fn load_index(&self) -> Result<ProfileIndex, ProfileError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(ProfileIndex::default());
        }
        let bytes = read_file(&path, "read profile index")?;
        let version = parse_version(&bytes, &path)?;
        if version != PROFILE_INDEX_SCHEMA_VERSION {
            return Err(ProfileError::UnsupportedIndexSchema {
                found: version,
                supported: PROFILE_INDEX_SCHEMA_VERSION,
            });
        }
        let index: ProfileIndex =
            serde_json::from_slice(&bytes).map_err(|error| ProfileError::InvalidJson {
                path: path.clone(),
                message: error.to_string(),
            })?;
        validate_index(&index)?;
        Ok(index)
    }

    fn write_index(&self, index: &ProfileIndex) -> Result<(), ProfileError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| io_error("create profile directory", &self.root, error))?;
        atomic_write_json(&self.index_path(), index)
    }

    fn write_catalog_if_absent(
        &self,
        fingerprint: &DatasetFingerprint,
        catalog: &Catalog,
    ) -> Result<(), ProfileError> {
        let path = self.catalog_path(fingerprint);
        if path.exists() {
            self.read_catalog(fingerprint)?;
            return Ok(());
        }
        let file = CatalogFile {
            schema_version: CATALOG_SCHEMA_VERSION,
            dataset_fingerprint: fingerprint.as_str().to_owned(),
            catalog: CatalogDto::from(catalog),
        };
        fs::create_dir_all(self.catalogs_dir())
            .map_err(|error| io_error("create catalog directory", &self.catalogs_dir(), error))?;
        atomic_write_json(&path, &file)
    }

    fn read_catalog(&self, fingerprint: &DatasetFingerprint) -> Result<Catalog, ProfileError> {
        let path = self.catalog_path(fingerprint);
        let bytes = read_file(&path, "read cached catalog")?;
        let version = parse_version(&bytes, &path)?;
        if version != CATALOG_SCHEMA_VERSION {
            return Err(ProfileError::UnsupportedCatalogSchema {
                found: version,
                supported: CATALOG_SCHEMA_VERSION,
            });
        }
        let file: CatalogFile =
            serde_json::from_slice(&bytes).map_err(|error| ProfileError::InvalidJson {
                path: path.clone(),
                message: error.to_string(),
            })?;
        if file.dataset_fingerprint != fingerprint.as_str() {
            return Err(ProfileError::CatalogFingerprintMismatch {
                expected: fingerprint.clone(),
                found: file.dataset_fingerprint,
            });
        }
        file.catalog.try_into()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanDocument {
    name: PlanName,
    dataset_profile: ProfileName,
    dataset_fingerprint: DatasetFingerprint,
    plan: FactoryPlan,
    dirty: bool,
}

impl PlanDocument {
    #[must_use]
    pub const fn new(
        name: PlanName,
        dataset_profile: ProfileName,
        dataset_fingerprint: DatasetFingerprint,
        plan: FactoryPlan,
    ) -> Self {
        Self {
            name,
            dataset_profile,
            dataset_fingerprint,
            plan,
            dirty: true,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &PlanName {
        &self.name
    }

    #[must_use]
    pub const fn dataset_profile(&self) -> &ProfileName {
        &self.dataset_profile
    }

    #[must_use]
    pub const fn dataset_fingerprint(&self) -> &DatasetFingerprint {
        &self.dataset_fingerprint
    }

    #[must_use]
    pub const fn plan(&self) -> &FactoryPlan {
        &self.plan
    }

    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn edit_plan<R>(&mut self, edit: impl FnOnce(&mut FactoryPlan) -> R) -> R {
        let result = edit(&mut self.plan);
        self.dirty = true;
        result
    }

    /// Applies a fallible edit and marks the document dirty only after success.
    ///
    /// # Errors
    ///
    /// Returns the closure's error and leaves the dirty flag unchanged when the
    /// edit fails.
    pub fn try_edit_plan<R, E>(
        &mut self,
        edit: impl FnOnce(&mut FactoryPlan) -> Result<R, E>,
    ) -> Result<R, E> {
        let result = edit(&mut self.plan)?;
        self.dirty = true;
        Ok(result)
    }

    fn loaded(
        name: PlanName,
        dataset_profile: ProfileName,
        dataset_fingerprint: DatasetFingerprint,
        plan: FactoryPlan,
    ) -> Self {
        Self {
            name,
            dataset_profile,
            dataset_fingerprint,
            plan,
            dirty: false,
        }
    }

    fn mark_clean(&mut self) {
        self.dirty = false;
    }

    fn bind_dataset(&mut self, profile: &DatasetProfile) {
        self.dataset_profile = profile.name().clone();
        self.dataset_fingerprint = profile.fingerprint().clone();
        self.dirty = true;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlanOpenResult {
    Ready(PlanDocument),
    Blocked(BlockedPlanDocument),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockedPlanDocument {
    document: PlanDocument,
    reason: PlanOpenBlockReason,
}

impl BlockedPlanDocument {
    #[must_use]
    pub const fn document(&self) -> &PlanDocument {
        &self.document
    }

    #[must_use]
    pub const fn reason(&self) -> &PlanOpenBlockReason {
        &self.reason
    }

    #[must_use]
    pub fn into_document(self) -> PlanDocument {
        self.document
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanOpenBlockReason {
    NamedProfileFingerprintMismatch {
        profile: ProfileName,
        expected: DatasetFingerprint,
        found: DatasetFingerprint,
    },
    DatasetProfileNotFound {
        profile: ProfileName,
        fingerprint: DatasetFingerprint,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MissingPlanReference {
    TargetCommodity(CommodityId),
    ExternalInput(CommodityId),
    RecipeChoiceCommodity(CommodityId),
    RecipeChoiceRecipe {
        commodity: CommodityId,
        recipe: RecipeId,
    },
    SourceChoiceCommodity(CommodityId),
    SourceChoiceSource {
        commodity: CommodityId,
        source: ProductionSource,
    },
    MachineChoiceRecipe(RecipeId),
    MachineChoiceMachine {
        recipe: RecipeId,
        machine: MachineId,
    },
    ModuleChoiceCommodity(CommodityId),
    ModuleChoiceModule {
        commodity: CommodityId,
        module: ModuleId,
    },
    FuelChoiceCommodity(CommodityId),
    FuelChoiceFuel {
        commodity: CommodityId,
        fuel: FuelId,
    },
    SelectedBelt(BeltId),
}

#[derive(Clone, Debug, Default)]
pub struct PlanFileStore;

impl PlanFileStore {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Saves a versioned factory plan atomically.
    ///
    /// # Errors
    ///
    /// Returns a structured error for invalid suffixes or filesystem failures.
    pub fn save(&self, path: &Path, document: &mut PlanDocument) -> Result<(), PlanFileError> {
        ensure_plan_suffix(path)?;
        let file = PlanFile::from(&*document);
        atomic_write_plan_json(path, &file)?;
        document.mark_clean();
        Ok(())
    }

    /// Loads a factory plan file without checking dataset availability.
    ///
    /// # Errors
    ///
    /// Returns a structured error for invalid suffixes, unsupported schema
    /// versions, invalid JSON, invalid domain values, or filesystem failures.
    pub fn load(&self, path: &Path) -> Result<PlanDocument, PlanFileError> {
        ensure_plan_suffix(path)?;
        let bytes = read_plan_file(path, "read plan file")?;
        let version = parse_plan_version(&bytes, path)?;
        if !(1..=PLAN_SCHEMA_VERSION).contains(&version) {
            return Err(PlanFileError::UnsupportedPlanSchema {
                found: version,
                supported: PLAN_SCHEMA_VERSION,
            });
        }
        serde_json::from_slice::<PlanFile>(&bytes)
            .map_err(|error| PlanFileError::InvalidJson {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?
            .into_document()
    }

    /// Opens a factory plan and validates its dataset binding against stored
    /// profiles.
    ///
    /// # Errors
    ///
    /// Returns a structured error for unreadable plan files or profile-store
    /// failures.
    pub fn open(
        &self,
        path: &Path,
        profiles: &ProfileStore,
    ) -> Result<PlanOpenResult, PlanFileError> {
        let mut document = self.load(path)?;
        match profiles.open(document.dataset_profile()) {
            Ok(profile) if profile.fingerprint() == document.dataset_fingerprint() => {
                Ok(PlanOpenResult::Ready(document))
            }
            Ok(profile) => Ok(PlanOpenResult::Blocked(BlockedPlanDocument {
                reason: PlanOpenBlockReason::NamedProfileFingerprintMismatch {
                    profile: document.dataset_profile().clone(),
                    expected: document.dataset_fingerprint().clone(),
                    found: profile.fingerprint().clone(),
                },
                document,
            })),
            Err(ProfileError::ProfileNotFound { .. }) => {
                if let Some(profile) =
                    profiles.find_by_fingerprint(document.dataset_fingerprint())?
                {
                    document.bind_dataset(&profile);
                    return Ok(PlanOpenResult::Ready(document));
                }
                Ok(PlanOpenResult::Blocked(BlockedPlanDocument {
                    reason: PlanOpenBlockReason::DatasetProfileNotFound {
                        profile: document.dataset_profile().clone(),
                        fingerprint: document.dataset_fingerprint().clone(),
                    },
                    document,
                }))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Explicitly binds a blocked plan to a different dataset profile after
    /// validating that persisted references still exist.
    ///
    /// # Errors
    ///
    /// Returns [`PlanFileError::MissingReferences`] when persisted IDs cannot
    /// be found in the candidate profile catalog.
    pub fn rebind(
        &self,
        blocked: BlockedPlanDocument,
        profile: &DatasetProfile,
    ) -> Result<PlanDocument, PlanFileError> {
        let mut document = blocked.into_document();
        let references = missing_plan_references(document.plan(), profile.catalog());
        if !references.is_empty() {
            return Err(PlanFileError::MissingReferences { references });
        }
        document.bind_dataset(profile);
        Ok(document)
    }
}

#[derive(Debug, Error)]
pub enum PlanFileError {
    #[error("plan file path must end with {expected_suffix}: {path}")]
    InvalidPlanSuffix {
        path: PathBuf,
        expected_suffix: &'static str,
    },
    #[error("plan schema version {found} is unsupported; current version is {supported}")]
    UnsupportedPlanSchema { found: u32, supported: u32 },
    #[error("invalid JSON in {path}: {message}")]
    InvalidJson { path: PathBuf, message: String },
    #[error("invalid plan file: {message}")]
    InvalidPlan { message: String },
    #[error("plan references are missing from the selected dataset: {references:?}")]
    MissingReferences {
        references: Vec<MissingPlanReference>,
    },
    #[error("{operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Profile(#[from] ProfileError),
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("the platform application-data directory is unavailable")]
    ApplicationDataDirectoryUnavailable,
    #[error("profile {name} already exists")]
    ProfileAlreadyExists { name: ProfileName },
    #[error("profile {name} was not found")]
    ProfileNotFound { name: ProfileName },
    #[error("profile index schema version {found} is unsupported; current version is {supported}")]
    UnsupportedIndexSchema { found: u32, supported: u32 },
    #[error("catalog schema version {found} is unsupported; current version is {supported}")]
    UnsupportedCatalogSchema { found: u32, supported: u32 },
    #[error("invalid JSON in {path}: {message}")]
    InvalidJson { path: PathBuf, message: String },
    #[error("invalid profile index: {message}")]
    InvalidIndex { message: String },
    #[error("invalid cached catalog: {message}")]
    InvalidCatalog { message: String },
    #[error("catalog fingerprint mismatch: expected {expected}, found {found}")]
    CatalogFingerprintMismatch {
        expected: DatasetFingerprint,
        found: String,
    },
    #[error("invalid stored dataset fingerprint {value}: {message}")]
    InvalidFingerprint { value: String, message: String },
    #[error("{operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error(transparent)]
    Locale(#[from] LocaleError),
}

#[derive(Debug)]
struct ImportedProfile {
    fingerprint: DatasetFingerprint,
    catalog: Catalog,
    metadata: ImportMetadata,
    diagnostics: Vec<ImportDiagnostic>,
}

impl ImportedProfile {
    fn into_profile(self, name: ProfileName) -> DatasetProfile {
        DatasetProfile {
            name,
            fingerprint: self.fingerprint,
            catalog: self.catalog,
            metadata: self.metadata,
            diagnostics: self.diagnostics,
        }
    }
}

fn import_request(request: &ProfileImportRequest) -> Result<ImportedProfile, ProfileError> {
    let (data_bytes, data_hash) = read_hashed_source(&request.data_path)?;
    let mut locale_contents = Vec::with_capacity(request.locale_paths.len());
    let mut locale_sources = Vec::with_capacity(request.locale_paths.len());
    for (kind, path) in &request.locale_paths {
        let (bytes, fingerprint) = read_hashed_source(path)?;
        locale_contents.push((*kind, bytes));
        locale_sources.push(LocaleSourceMetadata {
            kind: *kind,
            source: SourceMetadata {
                path: path.clone(),
                fingerprint,
            },
        });
    }

    let report = if locale_contents.is_empty() {
        parse_data_raw(Cursor::new(data_bytes))?
    } else {
        let locale = parse_prototype_locale(
            locale_contents
                .iter()
                .map(|(kind, bytes)| (*kind, Cursor::new(bytes.as_slice()))),
        )?;
        parse_data_raw_with_locale(Cursor::new(data_bytes), &locale)?
    };

    let fingerprint = calculate_dataset_fingerprint(
        IMPORTER_SCHEMA_VERSION,
        &data_hash,
        locale_sources
            .iter()
            .map(|source| (source.kind, source.source.fingerprint.as_str())),
    )?;
    let imported_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());

    Ok(ImportedProfile {
        fingerprint,
        catalog: report.catalog().clone(),
        metadata: ImportMetadata {
            imported_at_unix_seconds,
            importer_schema_version: IMPORTER_SCHEMA_VERSION,
            data_source: SourceMetadata {
                path: request.data_path.clone(),
                fingerprint: data_hash,
            },
            locale_sources,
        },
        diagnostics: report.diagnostics().to_vec(),
    })
}

fn read_hashed_source(path: &Path) -> Result<(Vec<u8>, String), ProfileError> {
    let file = File::open(path).map_err(|error| io_error("open import source", path, error))?;
    let mut reader = HashingReader::new(file);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read import source", path, error))?;
    Ok((bytes, reader.finalize().to_hex().to_string()))
}

struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: blake3::Hasher::new(),
        }
    }

    fn finalize(self) -> blake3::Hash {
        self.hasher.finalize()
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..length]);
        Ok(length)
    }
}

fn calculate_dataset_fingerprint<'a>(
    importer_schema_version: u32,
    data_hash: &str,
    locale_hashes: impl IntoIterator<Item = (LocalePrototypeKind, &'a str)>,
) -> Result<DatasetFingerprint, ProfileError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"factorio-planner-dataset-profile\0");
    hasher.update(&importer_schema_version.to_le_bytes());
    hasher.update(b"\0data\0");
    hasher.update(data_hash.as_bytes());
    for (kind, hash) in locale_hashes {
        hasher.update(b"\0locale\0");
        hasher.update(locale_kind_label(kind).as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
    }
    let fingerprint = hasher.finalize().to_hex();
    dataset_fingerprint(fingerprint.as_ref())
}

fn dataset_fingerprint(value: &str) -> Result<DatasetFingerprint, ProfileError> {
    DatasetFingerprint::new(value.to_owned()).map_err(|error| ProfileError::InvalidFingerprint {
        value: value.to_owned(),
        message: error.to_string(),
    })
}

fn locale_kind_label(kind: LocalePrototypeKind) -> &'static str {
    match kind {
        LocalePrototypeKind::Item => "item",
        LocalePrototypeKind::Fluid => "fluid",
        LocalePrototypeKind::Recipe => "recipe",
        LocalePrototypeKind::Entity => "entity",
    }
}

fn warning_count(diagnostics: &[ImportDiagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count()
}

fn read_file(path: &Path, operation: &'static str) -> Result<Vec<u8>, ProfileError> {
    fs::read(path).map_err(|error| io_error(operation, path, error))
}

fn parse_version(bytes: &[u8], path: &Path) -> Result<u32, ProfileError> {
    #[derive(Deserialize)]
    struct VersionProbe {
        schema_version: u32,
    }

    serde_json::from_slice::<VersionProbe>(bytes)
        .map(|probe| probe.schema_version)
        .map_err(|error| ProfileError::InvalidJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<(), ProfileError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| ProfileError::InvalidJson {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let parent = path.parent().ok_or_else(|| ProfileError::Io {
        operation: "resolve atomic-write parent",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create atomic-write directory", parent, error))?;
    let file_name = path.file_name().ok_or_else(|| ProfileError::Io {
        operation: "resolve atomic-write filename",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no filename"),
    })?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    let temporary_path = path.with_file_name(temporary_name);

    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| io_error("create temporary file", &temporary_path, error))?;
        file.write_all(&bytes)
            .map_err(|error| io_error("write temporary file", &temporary_path, error))?;
        file.flush()
            .map_err(|error| io_error("flush temporary file", &temporary_path, error))?;
        file.sync_all()
            .map_err(|error| io_error("sync temporary file", &temporary_path, error))?;
        drop(file);
        fs::rename(&temporary_path, path)
            .map_err(|error| io_error("replace destination file", path, error))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> ProfileError {
    ProfileError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProfileIndex {
    #[serde(default = "profile_index_schema_version")]
    schema_version: u32,
    active_profile: Option<String>,
    profiles: BTreeMap<String, StoredProfile>,
}

impl Default for ProfileIndex {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_INDEX_SCHEMA_VERSION,
            active_profile: None,
            profiles: BTreeMap::new(),
        }
    }
}

const fn profile_index_schema_version() -> u32 {
    PROFILE_INDEX_SCHEMA_VERSION
}

fn validate_index(index: &ProfileIndex) -> Result<(), ProfileError> {
    for name in index.profiles.keys() {
        ProfileName::new(name.clone()).map_err(|error| ProfileError::InvalidIndex {
            message: format!("invalid profile name {name:?}: {error}"),
        })?;
    }
    if let Some(active) = &index.active_profile
        && !index.profiles.contains_key(active)
    {
        return Err(ProfileError::InvalidIndex {
            message: format!("active profile {active} is not present"),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredProfile {
    dataset_fingerprint: String,
    metadata: ImportMetadataDto,
    diagnostics: Vec<ImportDiagnosticDto>,
    warning_count: usize,
}

impl StoredProfile {
    fn from_imported(imported: &ImportedProfile) -> Self {
        Self {
            dataset_fingerprint: imported.fingerprint.as_str().to_owned(),
            metadata: ImportMetadataDto::from(&imported.metadata),
            diagnostics: imported
                .diagnostics
                .iter()
                .map(ImportDiagnosticDto::from)
                .collect(),
            warning_count: warning_count(&imported.diagnostics),
        }
    }

    fn into_summary(self, name: String) -> Result<ProfileSummary, ProfileError> {
        let name = ProfileName::new(name).map_err(|error| ProfileError::InvalidIndex {
            message: error.to_string(),
        })?;
        let fingerprint = dataset_fingerprint(&self.dataset_fingerprint)?;
        let metadata = self.metadata.into_metadata()?;
        let diagnostics = self
            .diagnostics
            .into_iter()
            .map(ImportDiagnosticDto::into_diagnostic)
            .collect::<Vec<_>>();
        let actual_warning_count = warning_count(&diagnostics);
        if actual_warning_count != self.warning_count {
            return Err(ProfileError::InvalidIndex {
                message: format!(
                    "profile {name} stores warning count {}, but has {actual_warning_count} warnings",
                    self.warning_count
                ),
            });
        }
        Ok(ProfileSummary {
            name,
            fingerprint,
            metadata,
            warning_count: actual_warning_count,
        })
    }

    fn to_profile(
        &self,
        name: ProfileName,
        fingerprint: DatasetFingerprint,
        catalog: Catalog,
    ) -> Result<DatasetProfile, ProfileError> {
        let metadata = self.metadata.clone().into_metadata()?;
        let diagnostics = self
            .diagnostics
            .iter()
            .cloned()
            .map(ImportDiagnosticDto::into_diagnostic)
            .collect::<Vec<_>>();
        let actual_warning_count = warning_count(&diagnostics);
        if actual_warning_count != self.warning_count {
            return Err(ProfileError::InvalidIndex {
                message: format!(
                    "profile {name} stores warning count {}, but has {actual_warning_count} warnings",
                    self.warning_count
                ),
            });
        }
        Ok(DatasetProfile {
            name,
            fingerprint,
            catalog,
            metadata,
            diagnostics,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ImportMetadataDto {
    imported_at_unix_seconds: u64,
    importer_schema_version: u32,
    data_source: SourceMetadataDto,
    locale_sources: Vec<LocaleSourceMetadataDto>,
}

impl From<&ImportMetadata> for ImportMetadataDto {
    fn from(metadata: &ImportMetadata) -> Self {
        Self {
            imported_at_unix_seconds: metadata.imported_at_unix_seconds,
            importer_schema_version: metadata.importer_schema_version,
            data_source: SourceMetadataDto::from(&metadata.data_source),
            locale_sources: metadata
                .locale_sources
                .iter()
                .map(LocaleSourceMetadataDto::from)
                .collect(),
        }
    }
}

impl ImportMetadataDto {
    fn into_metadata(self) -> Result<ImportMetadata, ProfileError> {
        if self.importer_schema_version > IMPORTER_SCHEMA_VERSION {
            return Err(ProfileError::InvalidIndex {
                message: format!(
                    "importer schema version {} is newer than supported version {}",
                    self.importer_schema_version, IMPORTER_SCHEMA_VERSION
                ),
            });
        }
        let mut locale_sources = self
            .locale_sources
            .into_iter()
            .map(LocaleSourceMetadataDto::into_metadata)
            .collect::<Vec<_>>();
        locale_sources.sort_by_key(|source| source.kind);
        if locale_sources
            .windows(2)
            .any(|sources| sources[0].kind == sources[1].kind)
        {
            return Err(ProfileError::InvalidIndex {
                message: "duplicate locale source kind".into(),
            });
        }
        Ok(ImportMetadata {
            imported_at_unix_seconds: self.imported_at_unix_seconds,
            importer_schema_version: self.importer_schema_version,
            data_source: self.data_source.into_metadata(),
            locale_sources,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SourceMetadataDto {
    path: PathBuf,
    fingerprint: String,
}

impl From<&SourceMetadata> for SourceMetadataDto {
    fn from(metadata: &SourceMetadata) -> Self {
        Self {
            path: metadata.path.clone(),
            fingerprint: metadata.fingerprint.clone(),
        }
    }
}

impl SourceMetadataDto {
    fn into_metadata(self) -> SourceMetadata {
        SourceMetadata {
            path: self.path,
            fingerprint: self.fingerprint,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LocaleSourceMetadataDto {
    kind: LocaleKindDto,
    source: SourceMetadataDto,
}

impl From<&LocaleSourceMetadata> for LocaleSourceMetadataDto {
    fn from(metadata: &LocaleSourceMetadata) -> Self {
        Self {
            kind: metadata.kind.into(),
            source: SourceMetadataDto::from(&metadata.source),
        }
    }
}

impl LocaleSourceMetadataDto {
    fn into_metadata(self) -> LocaleSourceMetadata {
        LocaleSourceMetadata {
            kind: self.kind.into(),
            source: self.source.into_metadata(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LocaleKindDto {
    Item,
    Fluid,
    Recipe,
    Entity,
}

impl From<LocalePrototypeKind> for LocaleKindDto {
    fn from(kind: LocalePrototypeKind) -> Self {
        match kind {
            LocalePrototypeKind::Item => Self::Item,
            LocalePrototypeKind::Fluid => Self::Fluid,
            LocalePrototypeKind::Recipe => Self::Recipe,
            LocalePrototypeKind::Entity => Self::Entity,
        }
    }
}

impl From<LocaleKindDto> for LocalePrototypeKind {
    fn from(kind: LocaleKindDto) -> Self {
        match kind {
            LocaleKindDto::Item => Self::Item,
            LocaleKindDto::Fluid => Self::Fluid,
            LocaleKindDto::Recipe => Self::Recipe,
            LocaleKindDto::Entity => Self::Entity,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ImportDiagnosticDto {
    severity: DiagnosticSeverityDto,
    prototype_type: Option<String>,
    prototype_id: Option<String>,
    path: String,
    message: String,
    disposition: PrototypeDispositionDto,
}

impl From<&ImportDiagnostic> for ImportDiagnosticDto {
    fn from(diagnostic: &ImportDiagnostic) -> Self {
        Self {
            severity: diagnostic.severity.into(),
            prototype_type: diagnostic.prototype_type.clone(),
            prototype_id: diagnostic.prototype_id.clone(),
            path: diagnostic.path.clone(),
            message: diagnostic.message.clone(),
            disposition: diagnostic.disposition.into(),
        }
    }
}

impl ImportDiagnosticDto {
    fn into_diagnostic(self) -> ImportDiagnostic {
        ImportDiagnostic {
            severity: self.severity.into(),
            prototype_type: self.prototype_type,
            prototype_id: self.prototype_id,
            path: self.path,
            message: self.message,
            disposition: self.disposition.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticSeverityDto {
    Warning,
    Error,
}

impl From<DiagnosticSeverity> for DiagnosticSeverityDto {
    fn from(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::Warning => Self::Warning,
            DiagnosticSeverity::Error => Self::Error,
        }
    }
}

impl From<DiagnosticSeverityDto> for DiagnosticSeverity {
    fn from(severity: DiagnosticSeverityDto) -> Self {
        match severity {
            DiagnosticSeverityDto::Warning => Self::Warning,
            DiagnosticSeverityDto::Error => Self::Error,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PrototypeDispositionDto {
    Retained,
    PartiallyRetained,
    Rejected,
}

impl From<PrototypeDisposition> for PrototypeDispositionDto {
    fn from(disposition: PrototypeDisposition) -> Self {
        match disposition {
            PrototypeDisposition::Retained => Self::Retained,
            PrototypeDisposition::PartiallyRetained => Self::PartiallyRetained,
            PrototypeDisposition::Rejected => Self::Rejected,
        }
    }
}

impl From<PrototypeDispositionDto> for PrototypeDisposition {
    fn from(disposition: PrototypeDispositionDto) -> Self {
        match disposition {
            PrototypeDispositionDto::Retained => Self::Retained,
            PrototypeDispositionDto::PartiallyRetained => Self::PartiallyRetained,
            PrototypeDispositionDto::Rejected => Self::Rejected,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogFile {
    schema_version: u32,
    dataset_fingerprint: String,
    catalog: CatalogDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogDto {
    commodities: Vec<CommodityDto>,
    #[serde(default)]
    fluid_properties: Vec<FluidPropertiesDto>,
    recipes: Vec<RecipeDto>,
    #[serde(default)]
    resource_sources: Vec<ResourceSourceDto>,
    #[serde(default)]
    fluid_sources: Vec<FluidSourceDto>,
    #[serde(default)]
    rocket_launch_sources: Vec<RocketLaunchSourceDto>,
    machines: Vec<MachineDto>,
    #[serde(default)]
    mining_machines: Vec<MiningMachineDto>,
    modules: Vec<ModuleDto>,
    fuels: Vec<FuelDto>,
    belts: Vec<BeltDto>,
}

impl From<&Catalog> for CatalogDto {
    fn from(catalog: &Catalog) -> Self {
        Self {
            commodities: catalog.commodities().map(CommodityDto::from).collect(),
            fluid_properties: catalog
                .fluid_properties_iter()
                .map(FluidPropertiesDto::from)
                .collect(),
            recipes: catalog.recipes().map(RecipeDto::from).collect(),
            resource_sources: catalog
                .resource_sources()
                .map(ResourceSourceDto::from)
                .collect(),
            fluid_sources: catalog.fluid_sources().map(FluidSourceDto::from).collect(),
            rocket_launch_sources: catalog
                .rocket_launch_sources()
                .map(RocketLaunchSourceDto::from)
                .collect(),
            machines: catalog.machines().map(MachineDto::from).collect(),
            mining_machines: catalog
                .mining_machines()
                .map(MiningMachineDto::from)
                .collect(),
            modules: catalog.modules().map(ModuleDto::from).collect(),
            fuels: catalog.fuels().map(FuelDto::from).collect(),
            belts: catalog.belts().map(BeltDto::from).collect(),
        }
    }
}

impl TryFrom<CatalogDto> for Catalog {
    type Error = ProfileError;

    fn try_from(catalog: CatalogDto) -> Result<Self, Self::Error> {
        let parts = CatalogParts {
            commodities: catalog
                .commodities
                .into_iter()
                .map(CommodityDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            fluid_properties: catalog
                .fluid_properties
                .into_iter()
                .map(FluidPropertiesDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            recipes: catalog
                .recipes
                .into_iter()
                .map(RecipeDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            resource_sources: catalog
                .resource_sources
                .into_iter()
                .map(ResourceSourceDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            fluid_sources: catalog
                .fluid_sources
                .into_iter()
                .map(FluidSourceDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            rocket_launch_sources: catalog
                .rocket_launch_sources
                .into_iter()
                .map(RocketLaunchSourceDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            machines: catalog
                .machines
                .into_iter()
                .map(MachineDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            mining_machines: catalog
                .mining_machines
                .into_iter()
                .map(MiningMachineDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            modules: catalog
                .modules
                .into_iter()
                .map(ModuleDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            fuels: catalog
                .fuels
                .into_iter()
                .map(FuelDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            belts: catalog
                .belts
                .into_iter()
                .map(BeltDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
        };
        Catalog::try_from_parts(parts).map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
enum CommodityIdDto {
    Item(String),
    Fluid(String),
}

impl From<&CommodityId> for CommodityIdDto {
    fn from(id: &CommodityId) -> Self {
        match id {
            CommodityId::Item(id) => Self::Item(id.as_str().to_owned()),
            CommodityId::Fluid(id) => Self::Fluid(id.as_str().to_owned()),
        }
    }
}

impl CommodityIdDto {
    fn into_id(self) -> Result<CommodityId, ProfileError> {
        match self {
            Self::Item(id) => ItemId::new(id)
                .map(CommodityId::Item)
                .map_err(|error| invalid_catalog(error.to_string())),
            Self::Fluid(id) => FluidId::new(id)
                .map(CommodityId::Fluid)
                .map_err(|error| invalid_catalog(error.to_string())),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PlanFile {
    schema_version: u32,
    name: String,
    dataset_profile: String,
    dataset_fingerprint: String,
    plan: FactoryPlanDto,
}

impl From<&PlanDocument> for PlanFile {
    fn from(document: &PlanDocument) -> Self {
        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            name: document.name().as_str().to_owned(),
            dataset_profile: document.dataset_profile().as_str().to_owned(),
            dataset_fingerprint: document.dataset_fingerprint().as_str().to_owned(),
            plan: FactoryPlanDto::from(document.plan()),
        }
    }
}

impl PlanFile {
    fn into_document(self) -> Result<PlanDocument, PlanFileError> {
        let name = PlanName::new(self.name).map_err(|error| invalid_plan(error.to_string()))?;
        let dataset_profile = ProfileName::new(self.dataset_profile)
            .map_err(|error| invalid_plan(error.to_string()))?;
        let dataset_fingerprint = plan_dataset_fingerprint(self.dataset_fingerprint)?;
        Ok(PlanDocument::loaded(
            name,
            dataset_profile,
            dataset_fingerprint,
            self.plan.into_plan()?,
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FactoryPlanDto {
    targets: Vec<TargetDto>,
    external_inputs: Vec<CommodityIdDto>,
    #[serde(default)]
    source_choices: Vec<SourceChoiceDto>,
    #[serde(default)]
    recipe_choices: Vec<RecipeChoiceDto>,
    machine_choices: Vec<MachineChoiceDto>,
    module_choices: Vec<ModuleChoiceDto>,
    fuel_choices: Vec<FuelChoiceDto>,
    selected_belt: Option<String>,
    display_rate_unit: RateUnitDto,
}

impl From<&FactoryPlan> for FactoryPlanDto {
    fn from(plan: &FactoryPlan) -> Self {
        Self {
            targets: plan.targets().iter().map(TargetDto::from).collect(),
            external_inputs: plan
                .external_inputs()
                .iter()
                .map(CommodityIdDto::from)
                .collect(),
            source_choices: plan
                .source_choices()
                .iter()
                .map(|(commodity, source)| SourceChoiceDto {
                    commodity: commodity.into(),
                    source: source.into(),
                })
                .collect(),
            recipe_choices: plan
                .recipe_choices()
                .iter()
                .map(|(commodity, recipe)| RecipeChoiceDto {
                    commodity: commodity.into(),
                    recipe: recipe.as_str().to_owned(),
                })
                .collect(),
            machine_choices: plan
                .machine_choices()
                .iter()
                .map(|(recipe, machine)| MachineChoiceDto {
                    recipe: recipe.as_str().to_owned(),
                    machine: machine.as_str().to_owned(),
                })
                .collect(),
            module_choices: plan
                .module_choices()
                .iter()
                .map(|(commodity, modules)| ModuleChoiceDto {
                    commodity: commodity.into(),
                    modules: modules
                        .iter()
                        .map(|module| module.as_str().to_owned())
                        .collect(),
                })
                .collect(),
            fuel_choices: plan
                .fuel_choices()
                .iter()
                .map(|(commodity, fuel)| FuelChoiceDto {
                    commodity: commodity.into(),
                    fuel: fuel.as_str().to_owned(),
                })
                .collect(),
            selected_belt: plan.selected_belt().map(|belt| belt.as_str().to_owned()),
            display_rate_unit: plan.display_rate_unit().into(),
        }
    }
}

impl FactoryPlanDto {
    fn into_plan(self) -> Result<FactoryPlan, PlanFileError> {
        let mut targets = self.targets.into_iter();
        let first_target = targets
            .next()
            .ok_or_else(|| invalid_plan("plan must contain at least one target"))?
            .into_target()?;
        let mut plan = FactoryPlan::new(first_target);
        for target in targets {
            plan.add_target(target.into_target()?);
        }

        plan = plan
            .with_external_inputs(collect_unique_plan_commodities(
                self.external_inputs,
                "external input",
            )?)
            .with_display_rate_unit(self.display_rate_unit.into());
        if let Some(selected_belt) = self.selected_belt {
            plan.set_selected_belt(plan_belt_id(selected_belt)?);
        }

        for choice in self.source_choices {
            let commodity = plan_commodity_id(choice.commodity)?;
            let source = choice.source.into_source()?;
            if plan.set_source_choice(commodity.clone(), source).is_some() {
                return Err(invalid_plan(format!(
                    "duplicate source choice for commodity {commodity}"
                )));
            }
        }
        let mut recipe_choice_commodities = BTreeSet::new();
        for choice in self.recipe_choices {
            let commodity = plan_commodity_id(choice.commodity)?;
            let recipe = plan_recipe_id(choice.recipe)?;
            if !recipe_choice_commodities.insert(commodity.clone()) {
                return Err(invalid_plan(format!(
                    "duplicate recipe choice for commodity {commodity}"
                )));
            }
            match plan.source_choice(&commodity) {
                Some(ProductionSource::Recipe(selected)) if selected == &recipe => {}
                Some(_) => {
                    return Err(invalid_plan(format!(
                        "conflicting source and recipe choices for commodity {commodity}"
                    )));
                }
                None => {
                    plan.set_recipe_choice(commodity, recipe);
                }
            }
        }
        for choice in self.machine_choices {
            let recipe = plan_recipe_id(choice.recipe)?;
            let machine = plan_machine_id(choice.machine)?;
            if plan.set_machine_choice(recipe.clone(), machine).is_some() {
                return Err(invalid_plan(format!(
                    "duplicate machine choice for recipe {recipe}"
                )));
            }
        }
        for choice in self.module_choices {
            let commodity = plan_commodity_id(choice.commodity)?;
            let modules = choice
                .modules
                .into_iter()
                .map(plan_module_id)
                .collect::<Result<Vec<_>, _>>()?;
            if plan.set_modules(commodity.clone(), modules).is_some() {
                return Err(invalid_plan(format!(
                    "duplicate module choice for commodity {commodity}"
                )));
            }
        }
        for choice in self.fuel_choices {
            let commodity = plan_commodity_id(choice.commodity)?;
            let fuel = plan_fuel_id(choice.fuel)?;
            if plan.set_fuel_choice(commodity.clone(), fuel).is_some() {
                return Err(invalid_plan(format!(
                    "duplicate fuel choice for commodity {commodity}"
                )));
            }
        }
        Ok(plan)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TargetDto {
    commodity: CommodityIdDto,
    rate_per_second: f64,
}

impl From<&Target> for TargetDto {
    fn from(target: &Target) -> Self {
        Self {
            commodity: target.commodity().into(),
            rate_per_second: target.rate_per_second().get(),
        }
    }
}

impl TargetDto {
    fn into_target(self) -> Result<Target, PlanFileError> {
        Target::new(plan_commodity_id(self.commodity)?, self.rate_per_second)
            .map_err(|error| invalid_plan(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecipeChoiceDto {
    commodity: CommodityIdDto,
    recipe: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SourceChoiceDto {
    commodity: CommodityIdDto,
    source: ProductionSourceDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProductionSourceDto {
    Recipe { recipe: String },
    Resource { resource: String },
    Fluid { fluid_source: String },
    RocketLaunch { rocket_launch: String },
}

impl From<&ProductionSource> for ProductionSourceDto {
    fn from(source: &ProductionSource) -> Self {
        match source {
            ProductionSource::Recipe(recipe) => Self::Recipe {
                recipe: recipe.as_str().to_owned(),
            },
            ProductionSource::Resource(resource) => Self::Resource {
                resource: resource.as_str().to_owned(),
            },
            ProductionSource::Fluid(fluid_source) => Self::Fluid {
                fluid_source: fluid_source.as_str().to_owned(),
            },
            ProductionSource::RocketLaunch(rocket_launch) => Self::RocketLaunch {
                rocket_launch: rocket_launch.as_str().to_owned(),
            },
        }
    }
}

impl ProductionSourceDto {
    fn into_source(self) -> Result<ProductionSource, PlanFileError> {
        Ok(match self {
            Self::Recipe { recipe } => ProductionSource::Recipe(plan_recipe_id(recipe)?),
            Self::Resource { resource } => ProductionSource::Resource(
                ResourceSourceId::new(resource).map_err(|error| invalid_plan(error.to_string()))?,
            ),
            Self::Fluid { fluid_source } => ProductionSource::Fluid(
                FluidSourceId::new(fluid_source)
                    .map_err(|error| invalid_plan(error.to_string()))?,
            ),
            Self::RocketLaunch { rocket_launch } => ProductionSource::RocketLaunch(
                RocketLaunchSourceId::new(rocket_launch)
                    .map_err(|error| invalid_plan(error.to_string()))?,
            ),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MachineChoiceDto {
    recipe: String,
    machine: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModuleChoiceDto {
    commodity: CommodityIdDto,
    modules: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FuelChoiceDto {
    commodity: CommodityIdDto,
    fuel: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RateUnitDto {
    Second,
    Minute,
    Hour,
}

impl From<RateUnit> for RateUnitDto {
    fn from(unit: RateUnit) -> Self {
        match unit {
            RateUnit::Second => Self::Second,
            RateUnit::Minute => Self::Minute,
            RateUnit::Hour => Self::Hour,
        }
    }
}

impl From<RateUnitDto> for RateUnit {
    fn from(unit: RateUnitDto) -> Self {
        match unit {
            RateUnitDto::Second => Self::Second,
            RateUnitDto::Minute => Self::Minute,
            RateUnitDto::Hour => Self::Hour,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CommodityDto {
    id: CommodityIdDto,
    localized_name: Option<String>,
}

impl From<&Commodity> for CommodityDto {
    fn from(commodity: &Commodity) -> Self {
        Self {
            id: commodity.id().into(),
            localized_name: commodity.localized_name().map(str::to_owned),
        }
    }
}

impl CommodityDto {
    fn into_record(self) -> Result<Commodity, ProfileError> {
        Ok(Commodity::new(self.id.into_id()?, self.localized_name))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IngredientDto {
    commodity: CommodityIdDto,
    amount: f64,
}

impl From<&Ingredient> for IngredientDto {
    fn from(ingredient: &Ingredient) -> Self {
        Self {
            commodity: ingredient.commodity().into(),
            amount: ingredient.amount().get(),
        }
    }
}

impl IngredientDto {
    fn into_record(self) -> Result<Ingredient, ProfileError> {
        Ok(Ingredient::new(
            self.commodity.into_id()?,
            positive(self.amount)?,
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProductDto {
    commodity: CommodityIdDto,
    amount: f64,
    productivity_amount: f64,
}

impl From<&Product> for ProductDto {
    fn from(product: &Product) -> Self {
        Self {
            commodity: product.commodity().into(),
            amount: product.amount().get(),
            productivity_amount: product.productivity_amount().get(),
        }
    }
}

impl ProductDto {
    fn into_record(self) -> Result<Product, ProfileError> {
        Product::new(self.commodity.into_id()?, positive(self.amount)?)
            .with_productivity_amount(non_negative(self.productivity_amount)?)
            .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FluidPropertiesDto {
    fluid: String,
    default_temperature: f64,
    heat_capacity_joules_per_unit: f64,
}

impl From<&FluidProperties> for FluidPropertiesDto {
    fn from(properties: &FluidProperties) -> Self {
        Self {
            fluid: properties.fluid().as_str().to_owned(),
            default_temperature: properties.default_temperature().get(),
            heat_capacity_joules_per_unit: properties.heat_capacity_joules_per_unit().get(),
        }
    }
}

impl FluidPropertiesDto {
    fn into_record(self) -> Result<FluidProperties, ProfileError> {
        Ok(FluidProperties::new(
            fluid_id(self.fluid)?,
            non_negative(self.default_temperature)?,
            positive(self.heat_capacity_joules_per_unit)?,
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ResourceSourceDto {
    id: String,
    category: String,
    infinite: bool,
    mining_time: f64,
    products: Vec<ProductDto>,
    required_fluid: Option<IngredientDto>,
}

impl From<&ResourceSource> for ResourceSourceDto {
    fn from(source: &ResourceSource) -> Self {
        Self {
            id: source.id().as_str().to_owned(),
            category: source.category().as_str().to_owned(),
            infinite: source.infinite(),
            mining_time: source.mining_time().get(),
            products: source.products().iter().map(ProductDto::from).collect(),
            required_fluid: source.required_fluid().map(IngredientDto::from),
        }
    }
}

impl ResourceSourceDto {
    fn into_record(self) -> Result<ResourceSource, ProfileError> {
        ResourceSource::new(
            ResourceSourceId::new(self.id).map_err(|error| invalid_catalog(error.to_string()))?,
            ResourceCategory::new(self.category)
                .map_err(|error| invalid_catalog(error.to_string()))?,
            self.infinite,
            positive(self.mining_time)?,
            self.products
                .into_iter()
                .map(ProductDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            self.required_fluid
                .map(IngredientDto::into_record)
                .transpose()?,
        )
        .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FluidSourceKindDto {
    OffshorePump,
    BoilerSteam,
}

impl From<FluidSourceKind> for FluidSourceKindDto {
    fn from(kind: FluidSourceKind) -> Self {
        match kind {
            FluidSourceKind::OffshorePump => Self::OffshorePump,
            FluidSourceKind::BoilerSteam => Self::BoilerSteam,
        }
    }
}

impl From<FluidSourceKindDto> for FluidSourceKind {
    fn from(kind: FluidSourceKindDto) -> Self {
        match kind {
            FluidSourceKindDto::OffshorePump => Self::OffshorePump,
            FluidSourceKindDto::BoilerSteam => Self::BoilerSteam,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FluidSourceDto {
    id: String,
    kind: FluidSourceKindDto,
    products: Vec<ProductDto>,
    ingredients: Vec<IngredientDto>,
    energy_source: Option<MachineEnergySourceDto>,
    energy_usage: Option<f64>,
}

impl From<&FluidSource> for FluidSourceDto {
    fn from(source: &FluidSource) -> Self {
        Self {
            id: source.id().as_str().to_owned(),
            kind: source.kind().into(),
            products: source.products().iter().map(ProductDto::from).collect(),
            ingredients: source
                .ingredients()
                .iter()
                .map(IngredientDto::from)
                .collect(),
            energy_source: source.energy_source().map(MachineEnergySourceDto::from),
            energy_usage: source.energy_usage().map(Positive::get),
        }
    }
}

impl FluidSourceDto {
    fn into_record(self) -> Result<FluidSource, ProfileError> {
        FluidSource::new(
            FluidSourceId::new(self.id).map_err(|error| invalid_catalog(error.to_string()))?,
            self.kind.into(),
            self.products
                .into_iter()
                .map(ProductDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            self.ingredients
                .into_iter()
                .map(IngredientDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            self.energy_source
                .map(MachineEnergySourceDto::into_source)
                .transpose()?,
            self.energy_usage.map(positive).transpose()?,
        )
        .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RocketLaunchSourceDto {
    id: String,
    launched_item: String,
    products: Vec<ProductDto>,
    rocket_recipe: String,
    rocket_parts_required: f64,
}

impl From<&RocketLaunchSource> for RocketLaunchSourceDto {
    fn from(source: &RocketLaunchSource) -> Self {
        Self {
            id: source.id().as_str().to_owned(),
            launched_item: source.launched_item().as_str().to_owned(),
            products: source.products().iter().map(ProductDto::from).collect(),
            rocket_recipe: source.rocket_recipe().as_str().to_owned(),
            rocket_parts_required: source.rocket_parts_required().get(),
        }
    }
}

impl RocketLaunchSourceDto {
    fn into_record(self) -> Result<RocketLaunchSource, ProfileError> {
        RocketLaunchSource::new(
            RocketLaunchSourceId::new(self.id)
                .map_err(|error| invalid_catalog(error.to_string()))?,
            item_id(self.launched_item)?,
            self.products
                .into_iter()
                .map(ProductDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            recipe_id(self.rocket_recipe)?,
            positive(self.rocket_parts_required)?,
        )
        .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecipeDto {
    id: String,
    localized_name: Option<String>,
    category: String,
    duration: f64,
    ingredients: Vec<IngredientDto>,
    products: Vec<ProductDto>,
    main_product: Option<CommodityIdDto>,
    visible: bool,
    #[serde(default = "default_recipe_supported")]
    supported: bool,
    allowed_effects: Vec<ModuleEffectDto>,
    allowed_module_categories: Option<Vec<String>>,
    maximum_productivity: f64,
}

const fn default_recipe_supported() -> bool {
    true
}

impl From<&Recipe> for RecipeDto {
    fn from(recipe: &Recipe) -> Self {
        Self {
            id: recipe.id().as_str().to_owned(),
            localized_name: recipe.localized_name().map(str::to_owned),
            category: recipe.category().as_str().to_owned(),
            duration: recipe.duration().get(),
            ingredients: recipe
                .ingredients()
                .iter()
                .map(IngredientDto::from)
                .collect(),
            products: recipe.products().iter().map(ProductDto::from).collect(),
            main_product: recipe.main_product().map(CommodityIdDto::from),
            visible: recipe.visible(),
            supported: recipe.supported(),
            allowed_effects: recipe
                .allowed_effects()
                .iter()
                .copied()
                .map(ModuleEffectDto::from)
                .collect(),
            allowed_module_categories: recipe.allowed_module_categories().map(|categories| {
                categories
                    .iter()
                    .map(|category| category.as_str().to_owned())
                    .collect()
            }),
            maximum_productivity: recipe.maximum_productivity().get(),
        }
    }
}

impl RecipeDto {
    fn into_record(self) -> Result<Recipe, ProfileError> {
        let recipe = Recipe::new(
            recipe_id(self.id)?,
            recipe_category(self.category)?,
            positive(self.duration)?,
            self.ingredients
                .into_iter()
                .map(IngredientDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            self.products
                .into_iter()
                .map(ProductDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            self.main_product.map(CommodityIdDto::into_id).transpose()?,
            self.visible,
        )
        .map_err(|error| invalid_catalog(error.to_string()))?;
        let allowed_module_categories = self
            .allowed_module_categories
            .map(|categories| {
                categories
                    .into_iter()
                    .map(module_category)
                    .collect::<Result<BTreeSet<_>, _>>()
            })
            .transpose()?;

        Ok(recipe
            .with_supported(self.supported)
            .with_module_policy(
                self.allowed_effects.into_iter().map(ModuleEffect::from),
                allowed_module_categories,
                non_negative(self.maximum_productivity)?,
            )
            .with_localized_name(self.localized_name))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ModuleEffectDto {
    Speed,
    Productivity,
    Consumption,
    Pollution,
    Quality,
}

impl From<ModuleEffect> for ModuleEffectDto {
    fn from(effect: ModuleEffect) -> Self {
        match effect {
            ModuleEffect::Speed => Self::Speed,
            ModuleEffect::Productivity => Self::Productivity,
            ModuleEffect::Consumption => Self::Consumption,
            ModuleEffect::Pollution => Self::Pollution,
            ModuleEffect::Quality => Self::Quality,
        }
    }
}

impl From<ModuleEffectDto> for ModuleEffect {
    fn from(effect: ModuleEffectDto) -> Self {
        match effect {
            ModuleEffectDto::Speed => Self::Speed,
            ModuleEffectDto::Productivity => Self::Productivity,
            ModuleEffectDto::Consumption => Self::Consumption,
            ModuleEffectDto::Pollution => Self::Pollution,
            ModuleEffectDto::Quality => Self::Quality,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MachineEnergySourceDto {
    Electric {
        drain: f64,
    },
    Burner {
        fuel_categories: Vec<String>,
        effectivity: f64,
    },
    Unsupported {
        source: UnsupportedEnergySourceDto,
    },
}

impl From<&MachineEnergySource> for MachineEnergySourceDto {
    fn from(source: &MachineEnergySource) -> Self {
        match source {
            MachineEnergySource::Electric { drain } => Self::Electric { drain: drain.get() },
            MachineEnergySource::Burner {
                fuel_categories,
                effectivity,
            } => Self::Burner {
                fuel_categories: fuel_categories
                    .iter()
                    .map(|category| category.as_str().to_owned())
                    .collect(),
                effectivity: effectivity.get(),
            },
            MachineEnergySource::Unsupported(source) => Self::Unsupported {
                source: source.into(),
            },
        }
    }
}

impl MachineEnergySourceDto {
    fn into_source(self) -> Result<MachineEnergySource, ProfileError> {
        match self {
            Self::Electric { drain } => Ok(MachineEnergySource::Electric {
                drain: non_negative(drain)?,
            }),
            Self::Burner {
                fuel_categories,
                effectivity,
            } => Ok(MachineEnergySource::Burner {
                fuel_categories: fuel_categories
                    .into_iter()
                    .map(fuel_category)
                    .collect::<Result<BTreeSet<_>, _>>()?,
                effectivity: positive(effectivity)?,
            }),
            Self::Unsupported { source } => Ok(MachineEnergySource::Unsupported(source.into())),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum UnsupportedEnergySourceDto {
    Heat,
    Fluid,
    Void,
    Unknown(String),
}

impl From<&UnsupportedEnergySource> for UnsupportedEnergySourceDto {
    fn from(source: &UnsupportedEnergySource) -> Self {
        match source {
            UnsupportedEnergySource::Heat => Self::Heat,
            UnsupportedEnergySource::Fluid => Self::Fluid,
            UnsupportedEnergySource::Void => Self::Void,
            UnsupportedEnergySource::Unknown(value) => Self::Unknown(value.clone()),
        }
    }
}

impl From<UnsupportedEnergySourceDto> for UnsupportedEnergySource {
    fn from(source: UnsupportedEnergySourceDto) -> Self {
        match source {
            UnsupportedEnergySourceDto::Heat => Self::Heat,
            UnsupportedEnergySourceDto::Fluid => Self::Fluid,
            UnsupportedEnergySourceDto::Void => Self::Void,
            UnsupportedEnergySourceDto::Unknown(value) => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MachineDto {
    id: String,
    localized_name: Option<String>,
    crafting_categories: Vec<String>,
    crafting_speed: f64,
    module_slots: u16,
    allowed_effects: Vec<ModuleEffectDto>,
    allowed_module_categories: Option<Vec<String>>,
    energy_usage: f64,
    energy_source: MachineEnergySourceDto,
}

impl From<&Machine> for MachineDto {
    fn from(machine: &Machine) -> Self {
        Self {
            id: machine.id().as_str().to_owned(),
            localized_name: machine.localized_name().map(str::to_owned),
            crafting_categories: machine
                .crafting_categories()
                .iter()
                .map(|category| category.as_str().to_owned())
                .collect(),
            crafting_speed: machine.crafting_speed().get(),
            module_slots: machine.module_slots(),
            allowed_effects: machine
                .allowed_effects()
                .iter()
                .copied()
                .map(ModuleEffectDto::from)
                .collect(),
            allowed_module_categories: machine.allowed_module_categories().map(|categories| {
                categories
                    .iter()
                    .map(|category| category.as_str().to_owned())
                    .collect()
            }),
            energy_usage: machine.energy_usage().get(),
            energy_source: machine.energy_source().into(),
        }
    }
}

impl MachineDto {
    fn into_record(self) -> Result<Machine, ProfileError> {
        Machine::new(
            machine_id(self.id)?,
            self.crafting_categories
                .into_iter()
                .map(recipe_category)
                .collect::<Result<Vec<_>, _>>()?,
            positive(self.crafting_speed)?,
            self.module_slots,
            self.allowed_effects.into_iter().map(ModuleEffect::from),
            self.allowed_module_categories
                .map(|categories| {
                    categories
                        .into_iter()
                        .map(module_category)
                        .collect::<Result<BTreeSet<_>, _>>()
                })
                .transpose()?,
            positive(self.energy_usage)?,
            self.energy_source.into_source()?,
        )
        .map(|machine| machine.with_localized_name(self.localized_name))
        .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MiningMachineDto {
    id: String,
    localized_name: Option<String>,
    resource_categories: Vec<String>,
    mining_speed: f64,
    module_slots: u16,
    allowed_effects: Vec<ModuleEffectDto>,
    allowed_module_categories: Option<Vec<String>>,
    energy_usage: f64,
    energy_source: MachineEnergySourceDto,
}

impl From<&MiningMachine> for MiningMachineDto {
    fn from(mining_machine: &MiningMachine) -> Self {
        Self {
            id: mining_machine.id().as_str().to_owned(),
            localized_name: mining_machine.localized_name().map(str::to_owned),
            resource_categories: mining_machine
                .resource_categories()
                .iter()
                .map(|category| category.as_str().to_owned())
                .collect(),
            mining_speed: mining_machine.mining_speed().get(),
            module_slots: mining_machine.module_slots(),
            allowed_effects: mining_machine
                .allowed_effects()
                .iter()
                .copied()
                .map(ModuleEffectDto::from)
                .collect(),
            allowed_module_categories: mining_machine.allowed_module_categories().map(
                |categories| {
                    categories
                        .iter()
                        .map(|category| category.as_str().to_owned())
                        .collect()
                },
            ),
            energy_usage: mining_machine.energy_usage().get(),
            energy_source: mining_machine.energy_source().into(),
        }
    }
}

impl MiningMachineDto {
    fn into_record(self) -> Result<MiningMachine, ProfileError> {
        MiningMachine::new(
            mining_machine_id(self.id)?,
            self.resource_categories
                .into_iter()
                .map(resource_category)
                .collect::<Result<Vec<_>, _>>()?,
            positive(self.mining_speed)?,
            self.module_slots,
            self.allowed_effects.into_iter().map(ModuleEffect::from),
            self.allowed_module_categories
                .map(|categories| {
                    categories
                        .into_iter()
                        .map(module_category)
                        .collect::<Result<BTreeSet<_>, _>>()
                })
                .transpose()?,
            positive(self.energy_usage)?,
            self.energy_source.into_source()?,
        )
        .map(|mining_machine| mining_machine.with_localized_name(self.localized_name))
        .map_err(|error| invalid_catalog(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModuleDto {
    id: String,
    localized_name: Option<String>,
    category: String,
    speed_effect: f64,
    productivity_effect: f64,
    consumption_effect: f64,
    unsupported_effects: Vec<String>,
}

impl From<&Module> for ModuleDto {
    fn from(module: &Module) -> Self {
        Self {
            id: module.id().as_str().to_owned(),
            localized_name: module.localized_name().map(str::to_owned),
            category: module.category().as_str().to_owned(),
            speed_effect: module.speed_effect().get(),
            productivity_effect: module.productivity_effect().get(),
            consumption_effect: module.consumption_effect().get(),
            unsupported_effects: module.unsupported_effects().iter().cloned().collect(),
        }
    }
}

impl ModuleDto {
    fn into_record(self) -> Result<Module, ProfileError> {
        Ok(Module::new(
            module_id(self.id)?,
            module_category(self.category)?,
            finite(self.speed_effect)?,
            finite(self.productivity_effect)?,
            finite(self.consumption_effect)?,
        )
        .with_unsupported_effects(self.unsupported_effects)
        .with_localized_name(self.localized_name))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FuelDto {
    id: String,
    localized_name: Option<String>,
    item: String,
    category: String,
    value: f64,
    burnt_result: Option<String>,
}

impl From<&Fuel> for FuelDto {
    fn from(fuel: &Fuel) -> Self {
        Self {
            id: fuel.id().as_str().to_owned(),
            localized_name: fuel.localized_name().map(str::to_owned),
            item: fuel.item().as_str().to_owned(),
            category: fuel.category().as_str().to_owned(),
            value: fuel.fuel_value().get(),
            burnt_result: fuel.burnt_result().map(|item| item.as_str().to_owned()),
        }
    }
}

impl FuelDto {
    fn into_record(self) -> Result<Fuel, ProfileError> {
        Ok(Fuel::new(
            fuel_id(self.id)?,
            item_id(self.item)?,
            fuel_category(self.category)?,
            positive(self.value)?,
            self.burnt_result.map(item_id).transpose()?,
        )
        .with_localized_name(self.localized_name))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BeltDto {
    id: String,
    localized_name: Option<String>,
    throughput: f64,
}

impl From<&Belt> for BeltDto {
    fn from(belt: &Belt) -> Self {
        Self {
            id: belt.id().as_str().to_owned(),
            localized_name: belt.localized_name().map(str::to_owned),
            throughput: belt.throughput().get(),
        }
    }
}

impl BeltDto {
    fn into_record(self) -> Result<Belt, ProfileError> {
        Ok(Belt::new(belt_id(self.id)?, positive(self.throughput)?)
            .with_localized_name(self.localized_name))
    }
}

fn missing_plan_references(plan: &FactoryPlan, catalog: &Catalog) -> Vec<MissingPlanReference> {
    let mut references = BTreeSet::new();
    for target in plan.targets() {
        if catalog.commodity(target.commodity()).is_none() {
            references.insert(MissingPlanReference::TargetCommodity(
                target.commodity().clone(),
            ));
        }
    }
    for commodity in plan.external_inputs() {
        if catalog.commodity(commodity).is_none() {
            references.insert(MissingPlanReference::ExternalInput(commodity.clone()));
        }
    }
    for (commodity, recipe) in plan.recipe_choices() {
        if catalog.commodity(commodity).is_none() {
            references.insert(MissingPlanReference::RecipeChoiceCommodity(
                commodity.clone(),
            ));
        }
        if catalog.recipe(recipe).is_none() {
            references.insert(MissingPlanReference::RecipeChoiceRecipe {
                commodity: commodity.clone(),
                recipe: recipe.clone(),
            });
        }
    }
    for (commodity, source) in plan.source_choices() {
        if catalog.commodity(commodity).is_none() {
            references.insert(MissingPlanReference::SourceChoiceCommodity(
                commodity.clone(),
            ));
        }
        if source_missing(catalog, source) {
            references.insert(MissingPlanReference::SourceChoiceSource {
                commodity: commodity.clone(),
                source: source.clone(),
            });
        }
    }
    for (recipe, machine) in plan.machine_choices() {
        if catalog.recipe(recipe).is_none() {
            references.insert(MissingPlanReference::MachineChoiceRecipe(recipe.clone()));
        }
        if catalog.machine(machine).is_none() {
            references.insert(MissingPlanReference::MachineChoiceMachine {
                recipe: recipe.clone(),
                machine: machine.clone(),
            });
        }
    }
    for (commodity, modules) in plan.module_choices() {
        if catalog.commodity(commodity).is_none() {
            references.insert(MissingPlanReference::ModuleChoiceCommodity(
                commodity.clone(),
            ));
        }
        for module in modules {
            if catalog.module(module).is_none() {
                references.insert(MissingPlanReference::ModuleChoiceModule {
                    commodity: commodity.clone(),
                    module: module.clone(),
                });
            }
        }
    }
    for (commodity, fuel) in plan.fuel_choices() {
        if catalog.commodity(commodity).is_none() {
            references.insert(MissingPlanReference::FuelChoiceCommodity(commodity.clone()));
        }
        if catalog.fuel(fuel).is_none() {
            references.insert(MissingPlanReference::FuelChoiceFuel {
                commodity: commodity.clone(),
                fuel: fuel.clone(),
            });
        }
    }
    if let Some(belt) = plan.selected_belt()
        && catalog.belt(belt).is_none()
    {
        references.insert(MissingPlanReference::SelectedBelt(belt.clone()));
    }
    references.into_iter().collect()
}

fn collect_unique_plan_commodities(
    commodities: impl IntoIterator<Item = CommodityIdDto>,
    label: &'static str,
) -> Result<BTreeSet<CommodityId>, PlanFileError> {
    let mut unique = BTreeSet::new();
    for commodity in commodities {
        let commodity = plan_commodity_id(commodity)?;
        if !unique.insert(commodity.clone()) {
            return Err(invalid_plan(format!("duplicate {label} {commodity}")));
        }
    }
    Ok(unique)
}

fn source_missing(catalog: &Catalog, source: &ProductionSource) -> bool {
    match source {
        ProductionSource::Recipe(recipe) => catalog.recipe(recipe).is_none(),
        ProductionSource::Resource(resource) => catalog.resource_source(resource).is_none(),
        ProductionSource::Fluid(fluid_source) => catalog.fluid_source(fluid_source).is_none(),
        ProductionSource::RocketLaunch(rocket_launch) => {
            catalog.rocket_launch_source(rocket_launch).is_none()
        }
    }
}

fn ensure_plan_suffix(path: &Path) -> Result<(), PlanFileError> {
    if path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.ends_with(PLAN_FILE_SUFFIX))
    {
        return Ok(());
    }
    Err(PlanFileError::InvalidPlanSuffix {
        path: path.to_path_buf(),
        expected_suffix: PLAN_FILE_SUFFIX,
    })
}

fn read_plan_file(path: &Path, operation: &'static str) -> Result<Vec<u8>, PlanFileError> {
    fs::read(path).map_err(|error| plan_io_error(operation, path, error))
}

fn parse_plan_version(bytes: &[u8], path: &Path) -> Result<u32, PlanFileError> {
    #[derive(Deserialize)]
    struct VersionProbe {
        schema_version: u32,
    }

    serde_json::from_slice::<VersionProbe>(bytes)
        .map(|probe| probe.schema_version)
        .map_err(|error| PlanFileError::InvalidJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

fn atomic_write_plan_json(path: &Path, value: &impl Serialize) -> Result<(), PlanFileError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| PlanFileError::InvalidJson {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let parent = path.parent().ok_or_else(|| PlanFileError::Io {
        operation: "resolve atomic-write parent",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| plan_io_error("create atomic-write directory", parent, error))?;
    let file_name = path.file_name().ok_or_else(|| PlanFileError::Io {
        operation: "resolve atomic-write filename",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no filename"),
    })?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    let temporary_path = path.with_file_name(temporary_name);

    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| plan_io_error("create temporary file", &temporary_path, error))?;
        file.write_all(&bytes)
            .map_err(|error| plan_io_error("write temporary file", &temporary_path, error))?;
        file.flush()
            .map_err(|error| plan_io_error("flush temporary file", &temporary_path, error))?;
        file.sync_all()
            .map_err(|error| plan_io_error("sync temporary file", &temporary_path, error))?;
        drop(file);
        fs::rename(&temporary_path, path)
            .map_err(|error| plan_io_error("replace destination file", path, error))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn plan_io_error(operation: &'static str, path: &Path, source: io::Error) -> PlanFileError {
    PlanFileError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn invalid_plan(message: impl Into<String>) -> PlanFileError {
    PlanFileError::InvalidPlan {
        message: message.into(),
    }
}

fn plan_commodity_id(value: CommodityIdDto) -> Result<CommodityId, PlanFileError> {
    match value {
        CommodityIdDto::Item(id) => plan_item_id(id).map(CommodityId::Item),
        CommodityIdDto::Fluid(id) => plan_fluid_id(id).map(CommodityId::Fluid),
    }
}

fn plan_item_id(value: String) -> Result<ItemId, PlanFileError> {
    ItemId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_fluid_id(value: String) -> Result<FluidId, PlanFileError> {
    FluidId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_recipe_id(value: String) -> Result<RecipeId, PlanFileError> {
    RecipeId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_machine_id(value: String) -> Result<MachineId, PlanFileError> {
    MachineId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_module_id(value: String) -> Result<ModuleId, PlanFileError> {
    ModuleId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_fuel_id(value: String) -> Result<FuelId, PlanFileError> {
    FuelId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_belt_id(value: String) -> Result<BeltId, PlanFileError> {
    BeltId::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn plan_dataset_fingerprint(value: String) -> Result<DatasetFingerprint, PlanFileError> {
    DatasetFingerprint::new(value).map_err(|error| invalid_plan(error.to_string()))
}

fn invalid_catalog(message: String) -> ProfileError {
    ProfileError::InvalidCatalog { message }
}

fn item_id(value: String) -> Result<ItemId, ProfileError> {
    ItemId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn recipe_id(value: String) -> Result<RecipeId, ProfileError> {
    RecipeId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn machine_id(value: String) -> Result<MachineId, ProfileError> {
    MachineId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn mining_machine_id(value: String) -> Result<MiningMachineId, ProfileError> {
    MiningMachineId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn module_id(value: String) -> Result<ModuleId, ProfileError> {
    ModuleId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn fuel_id(value: String) -> Result<FuelId, ProfileError> {
    FuelId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn belt_id(value: String) -> Result<BeltId, ProfileError> {
    BeltId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn recipe_category(value: String) -> Result<RecipeCategory, ProfileError> {
    RecipeCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn resource_category(value: String) -> Result<ResourceCategory, ProfileError> {
    ResourceCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn module_category(value: String) -> Result<ModuleCategory, ProfileError> {
    ModuleCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn fuel_category(value: String) -> Result<FuelCategory, ProfileError> {
    FuelCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn fluid_id(value: String) -> Result<FluidId, ProfileError> {
    FluidId::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn positive(value: f64) -> Result<Positive, ProfileError> {
    Positive::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn non_negative(value: f64) -> Result<NonNegative, ProfileError> {
    NonNegative::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn finite(value: f64) -> Result<Finite, ProfileError> {
    Finite::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{LocalePrototypeKind, calculate_dataset_fingerprint};

    #[test]
    fn importer_schema_version_is_part_of_the_dataset_fingerprint() {
        let first =
            calculate_dataset_fingerprint(1, "data", [(LocalePrototypeKind::Item, "locale")])
                .unwrap();
        let second =
            calculate_dataset_fingerprint(2, "data", [(LocalePrototypeKind::Item, "locale")])
                .unwrap();

        assert_ne!(first, second);
    }
}
