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
    FluidId, Fuel, FuelCategory, FuelId, Ingredient, ItemId, Machine, MachineEnergySource,
    MachineId, Module, ModuleCategory, ModuleEffect, ModuleId, NonNegative, Positive, Product,
    Recipe, RecipeCategory, RecipeId, UnsupportedEnergySource,
};
use crate::import::{
    DiagnosticSeverity, ImportDiagnostic, ImportError, LocaleError, LocalePrototypeKind,
    PrototypeDisposition, parse_data_raw, parse_data_raw_with_locale, parse_prototype_locale,
};

pub const PROFILE_INDEX_SCHEMA_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const IMPORTER_SCHEMA_VERSION: u32 = 1;

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
    recipes: Vec<RecipeDto>,
    machines: Vec<MachineDto>,
    modules: Vec<ModuleDto>,
    fuels: Vec<FuelDto>,
    belts: Vec<BeltDto>,
}

impl From<&Catalog> for CatalogDto {
    fn from(catalog: &Catalog) -> Self {
        Self {
            commodities: catalog.commodities().map(CommodityDto::from).collect(),
            recipes: catalog.recipes().map(RecipeDto::from).collect(),
            machines: catalog.machines().map(MachineDto::from).collect(),
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
            recipes: catalog
                .recipes
                .into_iter()
                .map(RecipeDto::into_record)
                .collect::<Result<Vec<_>, _>>()?,
            machines: catalog
                .machines
                .into_iter()
                .map(MachineDto::into_record)
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
}

impl From<&Product> for ProductDto {
    fn from(product: &Product) -> Self {
        Self {
            commodity: product.commodity().into(),
            amount: product.amount().get(),
        }
    }
}

impl ProductDto {
    fn into_record(self) -> Result<Product, ProfileError> {
        Ok(Product::new(
            self.commodity.into_id()?,
            positive(self.amount)?,
        ))
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
        }
    }
}

impl RecipeDto {
    fn into_record(self) -> Result<Recipe, ProfileError> {
        Recipe::new(
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
        .map(|recipe| recipe.with_localized_name(self.localized_name))
        .map_err(|error| invalid_catalog(error.to_string()))
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

fn module_category(value: String) -> Result<ModuleCategory, ProfileError> {
    ModuleCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
}

fn fuel_category(value: String) -> Result<FuelCategory, ProfileError> {
    FuelCategory::new(value).map_err(|error| invalid_catalog(error.to_string()))
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
