use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::catalog::{
    Belt, BeltId, Catalog, CatalogParts, Commodity, CommodityId, Finite, FluidId, FluidProperties,
    FluidSource, FluidSourceId, FluidSourceKind, Fuel, FuelCategory, FuelId, Ingredient, ItemId,
    Machine, MachineEnergySource, MachineId, Module, ModuleCategory, ModuleEffect, ModuleId,
    NonNegative, Positive, Product, Recipe, RecipeCategory, RecipeId, ResourceCategory,
    ResourceSource, ResourceSourceId, RocketLaunchSource, RocketLaunchSourceId,
    UnsupportedEnergySource,
};

const DEFAULT_RECIPE_CATEGORY: &str = "crafting";
const DEFAULT_RECIPE_DURATION: f64 = 0.5;
const DEFAULT_MAXIMUM_PRODUCTIVITY: f64 = 3.0;
const MINIMUM_RECIPE_DURATION: f64 = 0.001;
const DEFAULT_BURNER_FUEL_CATEGORY: &str = "chemical";
const TICKS_PER_SECOND: f64 = 60.0;
const MAX_DISPLAYED_IMPORT_DIAGNOSTICS: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrototypeDisposition {
    Retained,
    PartiallyRetained,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDiagnostic {
    pub severity: DiagnosticSeverity,
    pub prototype_type: Option<String>,
    pub prototype_id: Option<String>,
    pub path: String,
    pub message: String,
    pub disposition: PrototypeDisposition,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportReport {
    catalog: Catalog,
    diagnostics: Vec<ImportDiagnostic>,
}

impl ImportReport {
    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[ImportDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_catalog(self) -> Catalog {
        self.catalog
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportError {
    Json {
        line: usize,
        column: usize,
        message: String,
    },
    InvalidData {
        diagnostics: Vec<ImportDiagnostic>,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json {
                line,
                column,
                message,
            } => {
                write!(
                    formatter,
                    "invalid JSON at line {line}, column {column}: {message}"
                )
            }
            Self::InvalidData { diagnostics } => {
                write!(
                    formatter,
                    "data.raw contains invalid supported prototype data"
                )?;
                write_import_diagnostic_summary(formatter, diagnostics)
            }
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Debug, Default, Deserialize)]
struct RelevantCollections {
    ammo: Option<Value>,
    armor: Option<Value>,
    blueprint: Option<Value>,
    #[serde(rename = "blueprint-book")]
    blueprint_book: Option<Value>,
    capsule: Option<Value>,
    #[serde(rename = "copy-paste-tool")]
    copy_paste_tool: Option<Value>,
    #[serde(rename = "deconstruction-item")]
    deconstruction_item: Option<Value>,
    item: Option<Value>,
    gun: Option<Value>,
    #[serde(rename = "item-with-entity-data")]
    item_with_entity_data: Option<Value>,
    #[serde(rename = "rail-planner")]
    rail_planner: Option<Value>,
    #[serde(rename = "repair-tool")]
    repair_tool: Option<Value>,
    #[serde(rename = "selection-tool")]
    selection_tool: Option<Value>,
    #[serde(rename = "spidertron-remote")]
    spidertron_remote: Option<Value>,
    tool: Option<Value>,
    #[serde(rename = "upgrade-item")]
    upgrade_item: Option<Value>,
    fluid: Option<Value>,
    module: Option<Value>,
    recipe: Option<Value>,
    resource: Option<Value>,
    #[serde(rename = "offshore-pump")]
    offshore_pump: Option<Value>,
    boiler: Option<Value>,
    #[serde(rename = "rocket-silo")]
    rocket_silo: Option<Value>,
    #[serde(rename = "assembling-machine")]
    assembling_machine: Option<Value>,
    furnace: Option<Value>,
    #[serde(rename = "transport-belt")]
    transport_belt: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LocalePrototypeKind {
    Item,
    Fluid,
    Recipe,
    Entity,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrototypeLocale {
    item: BTreeMap<String, String>,
    fluid: BTreeMap<String, String>,
    recipe: BTreeMap<String, String>,
    entity: BTreeMap<String, String>,
}

impl PrototypeLocale {
    #[must_use]
    pub fn localized_name(&self, kind: LocalePrototypeKind, id: &str) -> Option<&str> {
        self.names(kind).get(id).map(String::as_str)
    }

    fn names(&self, kind: LocalePrototypeKind) -> &BTreeMap<String, String> {
        match kind {
            LocalePrototypeKind::Item => &self.item,
            LocalePrototypeKind::Fluid => &self.fluid,
            LocalePrototypeKind::Recipe => &self.recipe,
            LocalePrototypeKind::Entity => &self.entity,
        }
    }

    fn names_mut(&mut self, kind: LocalePrototypeKind) -> &mut BTreeMap<String, String> {
        match kind {
            LocalePrototypeKind::Item => &mut self.item,
            LocalePrototypeKind::Fluid => &mut self.fluid,
            LocalePrototypeKind::Recipe => &mut self.recipe,
            LocalePrototypeKind::Entity => &mut self.entity,
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum LocaleError {
    #[error("invalid {prototype_kind:?} locale JSON at line {line}, column {column}: {message}")]
    Json {
        prototype_kind: LocalePrototypeKind,
        line: usize,
        column: usize,
        message: String,
    },
    #[error("invalid {prototype_kind:?} locale data at {path}: {message}")]
    InvalidData {
        prototype_kind: LocalePrototypeKind,
        path: String,
        message: String,
    },
}

/// Parses the `names` maps from Factorio's per-prototype locale dump files.
///
/// Missing `names` maps and unknown fields, including descriptions, are
/// ignored. Files for prototype kinds not needed by the planner do not need to
/// be supplied.
///
/// # Errors
///
/// Returns [`LocaleError::Json`] for invalid JSON and
/// [`LocaleError::InvalidData`] for malformed `names` maps.
pub fn parse_prototype_locale<R: Read>(
    files: impl IntoIterator<Item = (LocalePrototypeKind, R)>,
) -> Result<PrototypeLocale, LocaleError> {
    let mut locale = PrototypeLocale::default();
    for (kind, reader) in files {
        let mut deserializer = serde_json::Deserializer::from_reader(reader);
        let value = Value::deserialize(&mut deserializer)
            .map_err(|error| locale_json_error(kind, &error))?;
        deserializer
            .end()
            .map_err(|error| locale_json_error(kind, &error))?;
        let Value::Object(mut fields) = value else {
            return Err(LocaleError::InvalidData {
                prototype_kind: kind,
                path: String::new(),
                message: "locale file must be a JSON object".into(),
            });
        };
        let Some(names) = fields.remove("names") else {
            continue;
        };
        let Value::Object(names) = names else {
            return Err(LocaleError::InvalidData {
                prototype_kind: kind,
                path: "/names".into(),
                message: "names must be a JSON object".into(),
            });
        };
        for (id, name) in names {
            let Value::String(name) = name else {
                return Err(LocaleError::InvalidData {
                    prototype_kind: kind,
                    path: format!("/names/{}", pointer_segment(&id)),
                    message: "localized name must be a string".into(),
                });
            };
            locale.names_mut(kind).insert(id, name);
        }
    }
    Ok(locale)
}

fn locale_json_error(
    prototype_kind: LocalePrototypeKind,
    error: &serde_json::Error,
) -> LocaleError {
    LocaleError::Json {
        prototype_kind,
        line: error.line().max(1),
        column: error.column().max(1),
        message: error.to_string(),
    }
}

/// Parses the supported portion of a resolved Factorio `data.raw` JSON dump.
///
/// Unknown top-level prototype collections and unknown fields are ignored.
///
/// # Errors
///
/// Returns [`ImportError::Json`] for invalid JSON and
/// [`ImportError::InvalidData`] when a supported prototype is malformed.
pub fn parse_data_raw(reader: impl Read) -> Result<ImportReport, ImportError> {
    parse_data_raw_inner(reader, None)
}

/// Parses supported Factorio prototypes and attaches optional localized names.
///
/// Internal prototype names remain the authoritative IDs used by all catalog
/// records and indexes.
///
/// # Errors
///
/// Returns the same errors as [`parse_data_raw`].
pub fn parse_data_raw_with_locale(
    reader: impl Read,
    locale: &PrototypeLocale,
) -> Result<ImportReport, ImportError> {
    parse_data_raw_inner(reader, Some(locale))
}

fn parse_data_raw_inner(
    reader: impl Read,
    locale: Option<&PrototypeLocale>,
) -> Result<ImportReport, ImportError> {
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let raw =
        RelevantCollections::deserialize(&mut deserializer).map_err(|error| json_error(&error))?;
    deserializer.end().map_err(|error| json_error(&error))?;

    let mut diagnostics = Vec::new();
    let mut commodities = Vec::new();
    let mut parsed_fuels = Vec::new();
    let mut rocket_launch_items = Vec::new();
    parse_item_collection(
        raw.item,
        &mut commodities,
        &mut parsed_fuels,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "ammo",
        raw.ammo,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "armor",
        raw.armor,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "blueprint",
        raw.blueprint,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "blueprint-book",
        raw.blueprint_book,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "capsule",
        raw.capsule,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "copy-paste-tool",
        raw.copy_paste_tool,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "deconstruction-item",
        raw.deconstruction_item,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "gun",
        raw.gun,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "item-with-entity-data",
        raw.item_with_entity_data,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "rail-planner",
        raw.rail_planner,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "repair-tool",
        raw.repair_tool,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "selection-tool",
        raw.selection_tool,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "spidertron-remote",
        raw.spidertron_remote,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "tool",
        raw.tool,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    parse_item_like_collection(
        "upgrade-item",
        raw.upgrade_item,
        &mut commodities,
        &mut rocket_launch_items,
        &mut diagnostics,
        locale,
    );
    let fluid_properties =
        parse_fluid_collection(raw.fluid, &mut commodities, &mut diagnostics, locale);
    let modules = parse_module_collection(
        raw.module,
        &mut commodities,
        &mut parsed_fuels,
        &mut diagnostics,
        locale,
    );

    let commodity_ids = commodities
        .iter()
        .map(|commodity| commodity.id().clone())
        .collect::<BTreeSet<_>>();
    validate_fuel_references(&parsed_fuels, &commodity_ids, &mut diagnostics);
    let fuels = parsed_fuels.into_iter().map(|parsed| parsed.fuel).collect();
    let recipes = parse_recipe_collection(raw.recipe, &commodity_ids, &mut diagnostics, locale);
    let rocket_silo_requirements =
        parse_rocket_silo_collection(raw.rocket_silo, &recipes, &mut diagnostics);
    let rocket_launch_sources = parse_rocket_launch_sources(
        rocket_launch_items,
        rocket_silo_requirements.as_ref(),
        &commodity_ids,
        &mut diagnostics,
    );
    let resource_sources =
        parse_resource_collection(raw.resource, &commodity_ids, &mut diagnostics);
    let fluid_property_map = fluid_properties
        .iter()
        .map(|properties| (properties.fluid().clone(), properties.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut fluid_sources =
        parse_offshore_pump_collection(raw.offshore_pump, &commodity_ids, &mut diagnostics);
    fluid_sources.extend(parse_boiler_collection(
        raw.boiler,
        &commodity_ids,
        &fluid_property_map,
        &mut diagnostics,
    ));
    let mut machines = parse_machine_collection(
        "assembling-machine",
        raw.assembling_machine,
        &mut diagnostics,
        locale,
    );
    machines.extend(parse_machine_collection(
        "furnace",
        raw.furnace,
        &mut diagnostics,
        locale,
    ));
    let belts = parse_belt_collection(raw.transport_belt, &mut diagnostics, locale);

    if has_errors(&diagnostics) {
        return Err(ImportError::InvalidData { diagnostics });
    }

    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities,
        fluid_properties,
        recipes,
        resource_sources,
        fluid_sources,
        rocket_launch_sources,
        machines,
        modules,
        fuels,
        belts,
    })
    .map_err(|error| ImportError::InvalidData {
        diagnostics: vec![ImportDiagnostic {
            severity: DiagnosticSeverity::Error,
            prototype_type: None,
            prototype_id: None,
            path: String::new(),
            message: error.to_string(),
            disposition: PrototypeDisposition::Rejected,
        }],
    })?;

    Ok(ImportReport {
        catalog,
        diagnostics,
    })
}

fn json_error(error: &serde_json::Error) -> ImportError {
    ImportError::Json {
        line: error.line().max(1),
        column: error.column().max(1),
        message: error.to_string(),
    }
}

fn write_import_diagnostic_summary(
    formatter: &mut fmt::Formatter<'_>,
    diagnostics: &[ImportDiagnostic],
) -> fmt::Result {
    let error_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let mut shown = 0;

    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .take(MAX_DISPLAYED_IMPORT_DIAGNOSTICS)
    {
        shown += 1;
        write!(
            formatter,
            "\n  error at {}: {}",
            diagnostic.path, diagnostic.message
        )?;
    }

    let remaining = error_count.saturating_sub(shown);
    if remaining > 0 {
        write!(formatter, "\n  ... and {remaining} more errors")?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum CommodityKind {
    Item,
    Fluid,
}

impl CommodityKind {
    const fn prototype_type(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Fluid => "fluid",
        }
    }

    fn id(self, name: String) -> Result<CommodityId, crate::catalog::IdentifierError> {
        match self {
            Self::Item => ItemId::new(name).map(CommodityId::Item),
            Self::Fluid => FluidId::new(name).map(CommodityId::Fluid),
        }
    }
}

struct ParsedFuel {
    fuel: Fuel,
    prototype_type: &'static str,
}

struct ParsedRocketLaunchItem {
    prototype_type: &'static str,
    id: String,
    products: Value,
}

struct RocketSiloRequirements {
    rocket_recipe: RecipeId,
    rocket_parts_required: Positive,
}

fn parse_item_collection(
    collection: Option<Value>,
    commodities: &mut Vec<Commodity>,
    fuels: &mut Vec<ParsedFuel>,
    rocket_launch_items: &mut Vec<ParsedRocketLaunchItem>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) {
    let Some(collection) = collection else {
        return;
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("item"),
            None,
            "/item".into(),
            "prototype collection must be a JSON object",
        ));
        return;
    };

    for (id, prototype) in prototypes {
        let prototype_path = format!("/item/{}", pointer_segment(&id));
        let Value::Object(fields) = prototype else {
            diagnostics.push(error_diagnostic(
                Some("item"),
                Some(&id),
                prototype_path,
                "prototype must be a JSON object",
            ));
            continue;
        };

        let initial_errors = error_count(diagnostics);
        validate_prototype_identity(&fields, "item", &id, &prototype_path, diagnostics);
        let localized_name = locale_name(locale, LocalePrototypeKind::Item, &id);
        let item_id = ItemId::new(id.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "item",
                    &id,
                    format!("{prototype_path}/name"),
                    error.to_string(),
                );
                None
            },
            Some,
        );
        let fuel = item_id.as_ref().and_then(|item_id| {
            parse_fuel(
                "item",
                &id,
                item_id,
                &fields,
                &prototype_path,
                diagnostics,
                localized_name.clone(),
            )
        });

        if error_count(diagnostics) == initial_errors
            && let Some(item_id) = item_id
        {
            commodities.push(Commodity::new(CommodityId::Item(item_id), localized_name));
            if let Some(fuel) = fuel {
                fuels.push(fuel);
            }
            if let Some(products) = fields.get("rocket_launch_products") {
                rocket_launch_items.push(ParsedRocketLaunchItem {
                    prototype_type: "item",
                    id,
                    products: products.clone(),
                });
            }
        }
    }
}

fn parse_module_collection(
    collection: Option<Value>,
    commodities: &mut Vec<Commodity>,
    fuels: &mut Vec<ParsedFuel>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Vec<Module> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("module"),
            None,
            "/module".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    let mut modules = Vec::new();
    for (id, prototype) in prototypes {
        let prototype_path = format!("/module/{}", pointer_segment(&id));
        let Value::Object(fields) = prototype else {
            diagnostics.push(error_diagnostic(
                Some("module"),
                Some(&id),
                prototype_path,
                "prototype must be a JSON object",
            ));
            continue;
        };

        let initial_errors = error_count(diagnostics);
        validate_prototype_identity(&fields, "module", &id, &prototype_path, diagnostics);
        let localized_name = locale_name(locale, LocalePrototypeKind::Item, &id);
        let item_id = ItemId::new(id.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "module",
                    &id,
                    format!("{prototype_path}/name"),
                    error.to_string(),
                );
                None
            },
            Some,
        );
        let module_id = ModuleId::new(id.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "module",
                    &id,
                    format!("{prototype_path}/name"),
                    error.to_string(),
                );
                None
            },
            Some,
        );
        let category = parse_module_category(&fields, &id, &prototype_path, diagnostics);
        let effects = parse_module_effect_values(&fields, &id, &prototype_path, diagnostics);
        let fuel = item_id.as_ref().and_then(|item_id| {
            parse_fuel(
                "module",
                &id,
                item_id,
                &fields,
                &prototype_path,
                diagnostics,
                localized_name.clone(),
            )
        });

        if error_count(diagnostics) == initial_errors
            && let (Some(item_id), Some(module_id), Some(category), Some(effects)) =
                (item_id, module_id, category, effects)
        {
            commodities.push(Commodity::new(
                CommodityId::Item(item_id),
                localized_name.clone(),
            ));
            modules.push(
                Module::new(
                    module_id,
                    category,
                    effects.speed,
                    effects.productivity,
                    effects.consumption,
                )
                .with_unsupported_effects(effects.unsupported)
                .with_localized_name(localized_name),
            );
            if let Some(fuel) = fuel {
                fuels.push(fuel);
            }
        }
    }

    modules
}

fn parse_module_category(
    fields: &Map<String, Value>,
    module_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ModuleCategory> {
    let path = format!("{prototype_path}/category");
    let Some(value) = fields.get("category") else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            "missing required category",
        );
        return None;
    };
    let Value::String(category) = value else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            "category must be a string",
        );
        return None;
    };

    ModuleCategory::new(category).map_or_else(
        |error| {
            prototype_error(diagnostics, "module", module_id, path, error.to_string());
            None
        },
        Some,
    )
}

struct ParsedModuleEffects {
    speed: Finite,
    productivity: Finite,
    consumption: Finite,
    unsupported: BTreeSet<String>,
}

fn parse_module_effect_values(
    fields: &Map<String, Value>,
    module_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ParsedModuleEffects> {
    let path = format!("{prototype_path}/effect");
    let Some(value) = fields.get("effect") else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            "missing required effect",
        );
        return None;
    };
    let Value::Object(effects) = value else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            "effect must be an object",
        );
        return None;
    };

    let initial_errors = error_count(diagnostics);
    let speed = parse_module_effect_value(effects, "speed", module_id, &path, diagnostics);
    let productivity =
        parse_module_effect_value(effects, "productivity", module_id, &path, diagnostics);
    let consumption =
        parse_module_effect_value(effects, "consumption", module_id, &path, diagnostics);
    let mut unsupported = BTreeSet::new();
    for effect in effects.keys() {
        if !matches!(effect.as_str(), "speed" | "productivity" | "consumption") {
            unsupported.insert(effect.clone());
            diagnostics.push(warning_diagnostic(
                "module",
                module_id,
                format!("{path}/{}", pointer_segment(effect)),
                format!("unsupported module effect {effect:?} blocks module selection"),
            ));
        }
    }

    (error_count(diagnostics) == initial_errors).then(|| ParsedModuleEffects {
        speed: speed.expect("supported module effect parsed without errors"),
        productivity: productivity.expect("supported module effect parsed without errors"),
        consumption: consumption.expect("supported module effect parsed without errors"),
        unsupported,
    })
}

fn parse_module_effect_value(
    effects: &Map<String, Value>,
    effect: &str,
    module_id: &str,
    effect_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Finite> {
    let Some(value) = effects.get(effect) else {
        return Finite::new(0.0).ok();
    };
    let path = format!("{effect_path}/{}", pointer_segment(effect));
    let Value::Number(number) = value else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            format!("{effect} effect must be a number"),
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        prototype_error(
            diagnostics,
            "module",
            module_id,
            path,
            format!("{effect} effect must be a finite number"),
        );
        return None;
    };

    Finite::new(value).map_or_else(
        |error| {
            prototype_error(diagnostics, "module", module_id, path, error.to_string());
            None
        },
        Some,
    )
}

fn parse_fuel(
    prototype_type: &'static str,
    prototype_id: &str,
    item_id: &ItemId,
    fields: &Map<String, Value>,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
    localized_name: Option<String>,
) -> Option<ParsedFuel> {
    let fuel_value = parse_fuel_value(
        fields,
        prototype_type,
        prototype_id,
        prototype_path,
        diagnostics,
    )?;
    let category = parse_fuel_category(
        fields,
        prototype_type,
        prototype_id,
        prototype_path,
        diagnostics,
    )?;
    let initial_errors = error_count(diagnostics);
    let burnt_result = parse_burnt_result(
        fields,
        prototype_type,
        prototype_id,
        prototype_path,
        diagnostics,
    );
    if error_count(diagnostics) != initial_errors {
        return None;
    }
    let fuel_id = FuelId::new(prototype_id.to_owned()).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    )?;

    Some(ParsedFuel {
        fuel: Fuel::new(fuel_id, item_id.clone(), category, fuel_value, burnt_result)
            .with_localized_name(localized_name),
        prototype_type,
    })
}

fn parse_fuel_value(
    fields: &Map<String, Value>,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let value = fields.get("fuel_value")?;
    let fuel_value_path = format!("{prototype_path}/fuel_value");
    let fuel_value = match parse_energy_value(value, EnergyNormalization::Joules) {
        Ok(value) => value,
        Err(message) => {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                fuel_value_path,
                format!("invalid fuel_value: {message}"),
            );
            return None;
        }
    };
    if fuel_value == 0.0 {
        return None;
    }
    Positive::new(fuel_value).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                fuel_value_path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_fuel_category(
    fields: &Map<String, Value>,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<FuelCategory> {
    let category_path = format!("{prototype_path}/fuel_category");
    let Some(category) = fields.get("fuel_category") else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            category_path,
            "missing required fuel_category for positive fuel_value",
        );
        return None;
    };
    let Value::String(category) = category else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            category_path,
            "fuel_category must be a string",
        );
        return None;
    };
    FuelCategory::new(category).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                category_path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_burnt_result(
    fields: &Map<String, Value>,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ItemId> {
    let burnt_result_path = format!("{prototype_path}/burnt_result");
    match fields.get("burnt_result") {
        None => None,
        Some(Value::String(value)) if value.is_empty() => None,
        Some(Value::String(value)) => ItemId::new(value.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    prototype_type,
                    prototype_id,
                    burnt_result_path,
                    error.to_string(),
                );
                None
            },
            Some,
        ),
        Some(_) => {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                burnt_result_path,
                "burnt_result must be a string",
            );
            None
        }
    }
}

fn validate_fuel_references(
    fuels: &[ParsedFuel],
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    for parsed in fuels {
        let fuel = &parsed.fuel;
        if !commodities.contains(&CommodityId::Item(fuel.item().clone())) {
            prototype_error(
                diagnostics,
                parsed.prototype_type,
                fuel.id().as_str(),
                format!(
                    "/{}/{}/name",
                    parsed.prototype_type,
                    pointer_segment(fuel.id().as_str())
                ),
                format!("references missing item {:?}", fuel.item().as_str()),
            );
        }
        if let Some(burnt_result) = fuel.burnt_result()
            && !commodities.contains(&CommodityId::Item(burnt_result.clone()))
        {
            prototype_error(
                diagnostics,
                parsed.prototype_type,
                fuel.id().as_str(),
                format!(
                    "/{}/{}/burnt_result",
                    parsed.prototype_type,
                    pointer_segment(fuel.id().as_str())
                ),
                format!("references missing item {:?}", burnt_result.as_str()),
            );
        }
    }
}

fn parse_belt_collection(
    collection: Option<Value>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Vec<Belt> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("transport-belt"),
            None,
            "/transport-belt".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| parse_belt(&id, prototype, diagnostics, locale))
        .collect()
}

fn parse_belt(
    id: &str,
    prototype: Value,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Option<Belt> {
    let prototype_path = format!("/transport-belt/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("transport-belt"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };

    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, "transport-belt", id, &prototype_path, diagnostics);
    let belt_id = BeltId::new(id.to_owned()).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "transport-belt",
                id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    );
    let speed_path = format!("{prototype_path}/speed");
    let speed = match fields.get("speed") {
        Some(Value::Number(number)) => number.as_f64().or_else(|| {
            prototype_error(
                diagnostics,
                "transport-belt",
                id,
                speed_path.clone(),
                "speed must be a finite number",
            );
            None
        }),
        Some(_) => {
            prototype_error(
                diagnostics,
                "transport-belt",
                id,
                speed_path.clone(),
                "speed must be a number",
            );
            None
        }
        None => {
            prototype_error(
                diagnostics,
                "transport-belt",
                id,
                speed_path.clone(),
                "missing required speed",
            );
            None
        }
    };
    let throughput = speed.and_then(|speed| {
        Positive::new(speed * 480.0).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "transport-belt",
                    id,
                    speed_path,
                    format!("invalid belt throughput: {error}"),
                );
                None
            },
            Some,
        )
    });

    if error_count(diagnostics) != initial_errors {
        return None;
    }

    Some(
        Belt::new(belt_id?, throughput?).with_localized_name(locale_name(
            locale,
            LocalePrototypeKind::Entity,
            id,
        )),
    )
}

fn parse_fluid_collection(
    collection: Option<Value>,
    commodities: &mut Vec<Commodity>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Vec<FluidProperties> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("fluid"),
            None,
            "/fluid".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    let mut properties = Vec::new();
    for (id, prototype) in prototypes {
        let prototype_path = format!("/fluid/{}", pointer_segment(&id));
        let Value::Object(fields) = prototype else {
            diagnostics.push(error_diagnostic(
                Some("fluid"),
                Some(&id),
                prototype_path,
                "prototype must be a JSON object",
            ));
            continue;
        };

        let initial_errors = error_count(diagnostics);
        validate_prototype_identity(&fields, "fluid", &id, &prototype_path, diagnostics);
        let fluid_id = FluidId::new(id.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "fluid",
                    &id,
                    format!("{prototype_path}/name"),
                    error.to_string(),
                );
                None
            },
            Some,
        );
        let default_temperature = parse_optional_fluid_non_negative_number(
            &fields,
            "default_temperature",
            25.0,
            "fluid",
            &id,
            &prototype_path,
            diagnostics,
        );
        let heat_capacity = parse_fluid_heat_capacity(&fields, &id, &prototype_path, diagnostics);

        if error_count(diagnostics) == initial_errors
            && let (Some(fluid_id), Some(default_temperature), Some(heat_capacity)) =
                (fluid_id, default_temperature, heat_capacity)
        {
            commodities.push(Commodity::new(
                CommodityId::Fluid(fluid_id.clone()),
                locale_name(locale, LocalePrototypeKind::Fluid, &id),
            ));
            properties.push(FluidProperties::new(
                fluid_id,
                default_temperature,
                heat_capacity,
            ));
        }
    }
    properties
}

fn parse_optional_fluid_non_negative_number(
    fields: &Map<String, Value>,
    field: &str,
    default: f64,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<NonNegative> {
    let Some(value) = fields.get(field) else {
        return Some(NonNegative::new(default).expect("default non-negative value is valid"));
    };
    let path = format!("{prototype_path}/{field}");
    let Value::Number(number) = value else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            path,
            format!("{field} must be a number"),
        );
        return None;
    };
    let value = number.as_f64().unwrap_or(f64::NAN);
    NonNegative::new(value).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_fluid_heat_capacity(
    fields: &Map<String, Value>,
    fluid_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let Some(value) = fields.get("heat_capacity") else {
        return Positive::new(1_000.0).ok();
    };
    let path = format!("{prototype_path}/heat_capacity");
    parse_energy_value(value, EnergyNormalization::Joules)
        .and_then(|joules| Positive::new(joules).map_err(|error| error.to_string()))
        .map_or_else(
            |message| {
                prototype_error(
                    diagnostics,
                    "fluid",
                    fluid_id,
                    path,
                    format!("invalid heat_capacity: {message}"),
                );
                None
            },
            Some,
        )
}

fn parse_item_like_collection(
    collection_name: &'static str,
    collection: Option<Value>,
    commodities: &mut Vec<Commodity>,
    rocket_launch_items: &mut Vec<ParsedRocketLaunchItem>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) {
    let Some(collection) = collection else {
        return;
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some(collection_name),
            None,
            format!("/{collection_name}"),
            "prototype collection must be a JSON object",
        ));
        return;
    };

    for (id, prototype) in prototypes {
        let prototype_path = format!("/{collection_name}/{}", pointer_segment(&id));
        let Value::Object(fields) = prototype else {
            diagnostics.push(error_diagnostic(
                Some(collection_name),
                Some(&id),
                prototype_path,
                "prototype must be a JSON object",
            ));
            continue;
        };

        let initial_errors = diagnostics.len();
        validate_prototype_identity(&fields, collection_name, &id, &prototype_path, diagnostics);

        let item_id = match ItemId::new(id.clone()) {
            Ok(id) => Some(id),
            Err(error) => {
                diagnostics.push(error_diagnostic(
                    Some(collection_name),
                    Some(&id),
                    format!("{prototype_path}/name"),
                    error.to_string(),
                ));
                None
            }
        };

        if diagnostics.len() == initial_errors
            && let Some(item_id) = item_id
        {
            commodities.push(Commodity::new(
                CommodityId::Item(item_id),
                locale_name(locale, LocalePrototypeKind::Item, &id),
            ));
            if let Some(products) = fields.get("rocket_launch_products") {
                rocket_launch_items.push(ParsedRocketLaunchItem {
                    prototype_type: collection_name,
                    id,
                    products: products.clone(),
                });
            }
        }
    }
}

fn parse_recipe_collection(
    collection: Option<Value>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Vec<Recipe> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("recipe"),
            None,
            "/recipe".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| {
            if is_parameter_recipe(&prototype) || is_hidden_empty_placeholder_recipe(&prototype) {
                return None;
            }
            parse_recipe(&id, prototype, commodities, diagnostics, locale)
        })
        .collect()
}

fn parse_rocket_silo_collection(
    collection: Option<Value>,
    recipes: &[Recipe],
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RocketSiloRequirements> {
    let Some(collection) = collection else {
        return None;
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("rocket-silo"),
            None,
            "/rocket-silo".into(),
            "prototype collection must be a JSON object",
        ));
        return None;
    };
    let recipe_ids = recipes
        .iter()
        .map(|recipe| recipe.id().clone())
        .collect::<BTreeSet<_>>();

    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| parse_rocket_silo(&id, prototype, &recipe_ids, diagnostics))
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, requirements)| requirements)
}

fn parse_rocket_silo(
    id: &str,
    prototype: Value,
    recipe_ids: &BTreeSet<RecipeId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<(String, RocketSiloRequirements)> {
    let prototype_path = format!("/rocket-silo/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("rocket-silo"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };
    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, "rocket-silo", id, &prototype_path, diagnostics);
    let rocket_recipe =
        parse_rocket_silo_fixed_recipe(&fields, id, &prototype_path, recipe_ids, diagnostics);
    let rocket_parts_required = parse_positive_number_field(
        &fields,
        "rocket_parts_required",
        "rocket-silo",
        id,
        &prototype_path,
        diagnostics,
    );
    if error_count(diagnostics) == initial_errors {
        Some((
            id.to_owned(),
            RocketSiloRequirements {
                rocket_recipe: rocket_recipe?,
                rocket_parts_required: rocket_parts_required?,
            },
        ))
    } else {
        None
    }
}

fn parse_rocket_silo_fixed_recipe(
    fields: &Map<String, Value>,
    silo_id: &str,
    prototype_path: &str,
    recipe_ids: &BTreeSet<RecipeId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RecipeId> {
    let path = format!("{prototype_path}/fixed_recipe");
    let Some(value) = fields.get("fixed_recipe") else {
        prototype_error(
            diagnostics,
            "rocket-silo",
            silo_id,
            path,
            "missing required fixed_recipe",
        );
        return None;
    };
    let Value::String(recipe) = value else {
        prototype_error(
            diagnostics,
            "rocket-silo",
            silo_id,
            path,
            "fixed_recipe must be a string",
        );
        return None;
    };
    let recipe_id = RecipeId::new(recipe.clone()).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "rocket-silo",
                silo_id,
                path.clone(),
                error.to_string(),
            );
            None
        },
        Some,
    )?;
    if !recipe_ids.contains(&recipe_id) {
        prototype_error(
            diagnostics,
            "rocket-silo",
            silo_id,
            path,
            format!("references missing recipe {recipe:?}"),
        );
        return None;
    }
    Some(recipe_id)
}

fn parse_rocket_launch_sources(
    launch_items: Vec<ParsedRocketLaunchItem>,
    silo: Option<&RocketSiloRequirements>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Vec<RocketLaunchSource> {
    launch_items
        .into_iter()
        .filter_map(|launch_item| {
            parse_rocket_launch_source(launch_item, silo, commodities, diagnostics)
        })
        .collect()
}

fn parse_rocket_launch_source(
    launch_item: ParsedRocketLaunchItem,
    silo: Option<&RocketSiloRequirements>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RocketLaunchSource> {
    let prototype_path = format!(
        "/{}/{}",
        launch_item.prototype_type,
        pointer_segment(&launch_item.id)
    );
    let Some(silo) = silo else {
        diagnostics.push(warning_diagnostic(
            launch_item.prototype_type,
            &launch_item.id,
            format!("{prototype_path}/rocket_launch_products"),
            "rocket launch source is unsupported because no supported rocket silo requirements were imported",
        ));
        return None;
    };
    let Value::Array(entries) = &launch_item.products else {
        prototype_error(
            diagnostics,
            launch_item.prototype_type,
            &launch_item.id,
            format!("{prototype_path}/rocket_launch_products"),
            "rocket_launch_products must be an array",
        );
        return None;
    };
    if entries.is_empty() {
        prototype_error(
            diagnostics,
            launch_item.prototype_type,
            &launch_item.id,
            format!("{prototype_path}/rocket_launch_products"),
            "rocket_launch_products must contain at least one product",
        );
        return None;
    }

    let initial_errors = error_count(diagnostics);
    let mut products: Vec<Product> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{prototype_path}/rocket_launch_products/{index}");
        let entry_errors = error_count(diagnostics);
        let parsed = parse_product_entry(
            entry,
            &launch_item.id,
            &entry_path,
            commodities,
            diagnostics,
        );
        if error_count(diagnostics) == entry_errors
            && let Some((commodity, expected_amount, _)) = parsed
        {
            match Positive::new(expected_amount) {
                Ok(amount) => products.push(Product::new(commodity, amount)),
                Err(error) => prototype_error(
                    diagnostics,
                    launch_item.prototype_type,
                    &launch_item.id,
                    entry_path,
                    format!("expected output {error}"),
                ),
            }
        }
    }
    if error_count(diagnostics) != initial_errors {
        return None;
    }

    RocketLaunchSource::new(
        RocketLaunchSourceId::new(launch_item.id.clone()).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    launch_item.prototype_type,
                    &launch_item.id,
                    format!("{prototype_path}/name"),
                    error.to_string(),
                );
                None
            },
            Some,
        )?,
        ItemId::new(launch_item.id.clone()).expect("validated item IDs can be reused"),
        products,
        silo.rocket_recipe.clone(),
        silo.rocket_parts_required,
    )
    .map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                launch_item.prototype_type,
                &launch_item.id,
                prototype_path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_resource_collection(
    collection: Option<Value>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Vec<ResourceSource> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("resource"),
            None,
            "/resource".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| parse_resource(&id, prototype, commodities, diagnostics))
        .collect()
}

fn parse_resource(
    id: &str,
    prototype: Value,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ResourceSource> {
    let prototype_path = format!("/resource/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("resource"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };

    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, "resource", id, &prototype_path, diagnostics);
    warn_unsupported_resource_fields(&fields, id, &prototype_path, diagnostics);

    let resource_id = ResourceSourceId::new(id).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "resource",
                id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    );
    let category = parse_resource_category(&fields, id, &prototype_path, diagnostics);
    let infinite = parse_resource_infinite(&fields, id, &prototype_path, diagnostics);
    let minable = parse_resource_minable(&fields, id, &prototype_path, diagnostics);
    let (mining_time, products, required_fluid) =
        minable.as_ref().map_or((None, None, None), |minable| {
            (
                parse_resource_mining_time(minable, id, &prototype_path, diagnostics),
                parse_resource_products(minable, id, &prototype_path, commodities, diagnostics),
                parse_resource_required_fluid(
                    minable,
                    id,
                    &prototype_path,
                    commodities,
                    diagnostics,
                ),
            )
        });

    if error_count(diagnostics) != initial_errors {
        return None;
    }

    ResourceSource::new(
        resource_id?,
        category?,
        infinite?,
        mining_time?,
        products?,
        required_fluid?,
    )
    .map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "resource",
                id,
                prototype_path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn warn_unsupported_resource_fields(
    fields: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    for field in ["minimum", "normal"] {
        if fields.contains_key(field) {
            diagnostics.push(warning_diagnostic(
                "resource",
                resource_id,
                format!("{prototype_path}/{field}"),
                format!("unsupported resource field {field:?} is not used for extraction rates"),
            ));
        }
    }
}

fn parse_resource_category(
    fields: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ResourceCategory> {
    let category = match fields.get("category") {
        None => "basic-solid",
        Some(Value::String(category)) => category,
        Some(_) => {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                format!("{prototype_path}/category"),
                "category must be a string",
            );
            return None;
        }
    };
    ResourceCategory::new(category).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                format!("{prototype_path}/category"),
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_resource_infinite(
    fields: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<bool> {
    match fields.get("infinite") {
        None => Some(false),
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                format!("{prototype_path}/infinite"),
                "infinite must be a boolean",
            );
            None
        }
    }
}

fn parse_resource_minable<'a>(
    fields: &'a Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<&'a Map<String, Value>> {
    let path = format!("{prototype_path}/minable");
    match fields.get("minable") {
        Some(Value::Object(minable)) => Some(minable),
        Some(_) => {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                path,
                "minable must be an object",
            );
            None
        }
        None => {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                path,
                "missing required minable",
            );
            None
        }
    }
}

fn parse_resource_mining_time(
    minable: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let path = format!("{prototype_path}/minable/mining_time");
    let Some(value) = minable.get("mining_time") else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            "missing required mining_time",
        );
        return None;
    };
    let Value::Number(number) = value else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            "mining_time must be a number",
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            "mining_time must be a finite number",
        );
        return None;
    };
    Positive::new(value).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_resource_products(
    minable: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Vec<Product>> {
    if minable.contains_key("results") {
        let mut fields = Map::new();
        fields.insert(
            "results".to_owned(),
            minable
                .get("results")
                .expect("contains_key checked above")
                .clone(),
        );
        return parse_products(
            &fields,
            resource_id,
            &format!("{prototype_path}/minable"),
            commodities,
            diagnostics,
        );
    }

    let path = format!("{prototype_path}/minable/result");
    let Some(Value::String(result)) = minable.get("result") else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            "missing required result or results",
        );
        return None;
    };
    let commodity = ItemId::new(result.clone())
        .map(CommodityId::Item)
        .map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "resource",
                    resource_id,
                    path.clone(),
                    error.to_string(),
                );
                None
            },
            Some,
        )?;
    if !commodities.contains(&commodity) {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            format!("references missing item {result:?}"),
        );
        return None;
    }

    let amount = match minable.get("amount") {
        None => Positive::new(1.0).expect("default resource amount is valid"),
        Some(Value::Number(number)) => {
            let amount = number.as_f64().unwrap_or(f64::NAN);
            Positive::new(amount).map_or_else(
                |error| {
                    prototype_error(
                        diagnostics,
                        "resource",
                        resource_id,
                        format!("{prototype_path}/minable/amount"),
                        error.to_string(),
                    );
                    None
                },
                Some,
            )?
        }
        Some(_) => {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                format!("{prototype_path}/minable/amount"),
                "amount must be a number",
            );
            return None;
        }
    };
    Some(vec![Product::new(commodity, amount)])
}

fn parse_resource_required_fluid(
    minable: &Map<String, Value>,
    resource_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Option<Ingredient>> {
    let Some(required_fluid) = minable.get("required_fluid") else {
        return Some(None);
    };
    let initial_errors = error_count(diagnostics);
    let path = format!("{prototype_path}/minable/required_fluid");
    let commodity = if let Value::String(required_fluid) = required_fluid {
        let commodity = FluidId::new(required_fluid.clone())
            .map(CommodityId::Fluid)
            .map_or_else(
                |error| {
                    prototype_error(
                        diagnostics,
                        "resource",
                        resource_id,
                        path.clone(),
                        error.to_string(),
                    );
                    None
                },
                Some,
            );
        if let Some(commodity) = &commodity
            && !commodities.contains(commodity)
        {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                path,
                format!("references missing fluid {required_fluid:?}"),
            );
        }
        commodity
    } else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            path,
            "required_fluid must be a string",
        );
        None
    };

    let amount_path = format!("{prototype_path}/minable/fluid_amount");
    let amount = if let Some(amount) = minable.get("fluid_amount") {
        let Value::Number(number) = amount else {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                amount_path,
                "fluid_amount must be a number",
            );
            return None;
        };
        let Some(amount) = number.as_f64() else {
            prototype_error(
                diagnostics,
                "resource",
                resource_id,
                amount_path,
                "fluid_amount must be a finite number",
            );
            return None;
        };
        Positive::new(amount).map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    "resource",
                    resource_id,
                    amount_path,
                    error.to_string(),
                );
                None
            },
            Some,
        )
    } else {
        prototype_error(
            diagnostics,
            "resource",
            resource_id,
            amount_path,
            "missing required fluid_amount",
        );
        None
    };
    if error_count(diagnostics) != initial_errors {
        None
    } else {
        Some(Some(Ingredient::new(commodity?, amount?)))
    }
}

fn parse_offshore_pump_collection(
    collection: Option<Value>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Vec<FluidSource> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("offshore-pump"),
            None,
            "/offshore-pump".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };
    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| parse_offshore_pump(&id, prototype, commodities, diagnostics))
        .collect()
}

fn parse_offshore_pump(
    id: &str,
    prototype: Value,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<FluidSource> {
    let prototype_path = format!("/offshore-pump/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("offshore-pump"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };
    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, "offshore-pump", id, &prototype_path, diagnostics);
    let source_id = FluidSourceId::new(id).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "offshore-pump",
                id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    );
    let fluid = parse_offshore_pump_fluid(&fields, id, &prototype_path, commodities, diagnostics);
    let pumping_speed = parse_positive_number_field(
        &fields,
        "pumping_speed",
        "offshore-pump",
        id,
        &prototype_path,
        diagnostics,
    );
    if error_count(diagnostics) != initial_errors {
        return None;
    }
    let product_amount = Positive::new(pumping_speed?.get() * TICKS_PER_SECOND)
        .expect("positive pumping speed has positive per-second output");
    FluidSource::new(
        source_id?,
        FluidSourceKind::OffshorePump,
        vec![Product::new(fluid?, product_amount)],
        Vec::new(),
        None,
        None,
    )
    .map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "offshore-pump",
                id,
                prototype_path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_offshore_pump_fluid(
    fields: &Map<String, Value>,
    pump_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<CommodityId> {
    if matches!(fields.get("fluid"), Some(Value::String(_))) {
        return parse_filtered_fluid(
            fields,
            "fluid",
            "offshore-pump",
            pump_id,
            prototype_path,
            commodities,
            diagnostics,
        );
    }
    if let Some(Value::Object(fluid_box)) = fields.get("fluid_box")
        && matches!(fluid_box.get("filter"), Some(Value::String(_)))
    {
        return parse_filtered_fluid(
            fluid_box,
            "filter",
            "offshore-pump",
            pump_id,
            &format!("{prototype_path}/fluid_box"),
            commodities,
            diagnostics,
        );
    }

    let water = CommodityId::Fluid(FluidId::new("water").expect("water is a valid fluid ID"));
    if commodities.contains(&water) {
        return Some(water);
    }

    prototype_error(
        diagnostics,
        "offshore-pump",
        pump_id,
        format!("{prototype_path}/fluid"),
        "missing required fluid, fluid_box.filter, or water fluid commodity",
    );
    None
}

fn parse_boiler_collection(
    collection: Option<Value>,
    commodities: &BTreeSet<CommodityId>,
    fluid_properties: &BTreeMap<FluidId, FluidProperties>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Vec<FluidSource> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some("boiler"),
            None,
            "/boiler".into(),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };
    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| {
            parse_boiler(&id, prototype, commodities, fluid_properties, diagnostics)
        })
        .collect()
}

fn parse_boiler(
    id: &str,
    prototype: Value,
    commodities: &BTreeSet<CommodityId>,
    fluid_properties: &BTreeMap<FluidId, FluidProperties>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<FluidSource> {
    let prototype_path = format!("/boiler/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("boiler"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };
    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, "boiler", id, &prototype_path, diagnostics);
    let source_id = FluidSourceId::new(id).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                "boiler",
                id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    );
    let input_fluid = parse_fluid_box_filter(
        &fields,
        "fluid_box",
        "boiler",
        id,
        &prototype_path,
        commodities,
        diagnostics,
    );
    let output_fluid = parse_fluid_box_filter(
        &fields,
        "output_fluid_box",
        "boiler",
        id,
        &prototype_path,
        commodities,
        diagnostics,
    );
    let target_temperature = parse_positive_number_field(
        &fields,
        "target_temperature",
        "boiler",
        id,
        &prototype_path,
        diagnostics,
    );
    let energy_usage = parse_boiler_energy_consumption(&fields, id, &prototype_path, diagnostics);
    let energy_source = parse_machine_energy_source(
        &fields,
        energy_usage,
        "boiler",
        id,
        &prototype_path,
        diagnostics,
    );
    if error_count(diagnostics) != initial_errors {
        return None;
    }
    let energy_source = energy_source?;
    if !matches!(energy_source, MachineEnergySource::Burner { .. }) {
        diagnostics.push(warning_diagnostic(
            "boiler",
            id,
            format!("{prototype_path}/energy_source/type"),
            "boiler steam source is unsupported because only burner boilers are modeled",
        ));
        return None;
    }
    let input_fluid = input_fluid?;
    let output_fluid = output_fluid?;
    let CommodityId::Fluid(input_fluid_id) = input_fluid.clone() else {
        unreachable!("parse_fluid_box_filter only returns fluids")
    };
    let properties = fluid_properties.get(&input_fluid_id).expect(
        "fluid properties are parsed for every imported fluid commodity before boiler sources",
    );
    let delta_temperature = target_temperature?.get() - properties.default_temperature().get();
    let Ok(delta_temperature) = Positive::new(delta_temperature) else {
        prototype_error(
            diagnostics,
            "boiler",
            id,
            format!("{prototype_path}/target_temperature"),
            "target_temperature must be greater than the input fluid default_temperature",
        );
        return None;
    };
    let energy_per_unit =
        properties.heat_capacity_joules_per_unit().get() * delta_temperature.get();
    let output_rate = Positive::new(energy_usage?.get() / energy_per_unit)
        .expect("positive energy and temperature delta have positive output rate");
    FluidSource::new(
        source_id?,
        FluidSourceKind::BoilerSteam,
        vec![Product::new(output_fluid, output_rate)],
        vec![Ingredient::new(input_fluid, output_rate)],
        Some(energy_source),
        Some(energy_usage?),
    )
    .map_or_else(
        |error| {
            prototype_error(diagnostics, "boiler", id, prototype_path, error.to_string());
            None
        },
        Some,
    )
}

fn parse_boiler_energy_consumption(
    fields: &Map<String, Value>,
    boiler_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let path = format!("{prototype_path}/energy_consumption");
    let Some(value) = fields.get("energy_consumption") else {
        prototype_error(
            diagnostics,
            "boiler",
            boiler_id,
            path,
            "missing required energy_consumption",
        );
        return None;
    };
    parse_energy_value(value, EnergyNormalization::Watts)
        .and_then(|watts| Positive::new(watts).map_err(|error| error.to_string()))
        .map_or_else(
            |message| {
                prototype_error(
                    diagnostics,
                    "boiler",
                    boiler_id,
                    path,
                    format!("invalid energy_consumption: {message}"),
                );
                None
            },
            Some,
        )
}

fn parse_fluid_box_filter(
    fields: &Map<String, Value>,
    field: &str,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<CommodityId> {
    let path = format!("{prototype_path}/{field}");
    let Some(Value::Object(fluid_box)) = fields.get(field) else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            path,
            format!("missing required {field} object"),
        );
        return None;
    };
    parse_filtered_fluid(
        fluid_box,
        "filter",
        prototype_type,
        prototype_id,
        &path,
        commodities,
        diagnostics,
    )
}

fn parse_filtered_fluid(
    fields: &Map<String, Value>,
    field: &str,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<CommodityId> {
    let path = format!("{prototype_path}/{field}");
    let Some(Value::String(fluid)) = fields.get(field) else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            path,
            format!("missing required {field} fluid"),
        );
        return None;
    };
    let commodity = FluidId::new(fluid.clone())
        .map(CommodityId::Fluid)
        .map_or_else(
            |error| {
                prototype_error(
                    diagnostics,
                    prototype_type,
                    prototype_id,
                    path.clone(),
                    error.to_string(),
                );
                None
            },
            Some,
        )?;
    if !commodities.contains(&commodity) {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            path,
            format!("references missing fluid {fluid:?}"),
        );
        return None;
    }
    Some(commodity)
}

fn parse_positive_number_field(
    fields: &Map<String, Value>,
    field: &str,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let path = format!("{prototype_path}/{field}");
    let Some(Value::Number(number)) = fields.get(field) else {
        prototype_error(
            diagnostics,
            prototype_type,
            prototype_id,
            path,
            format!("missing required {field} number"),
        );
        return None;
    };
    let value = number.as_f64().unwrap_or(f64::NAN);
    Positive::new(value).map_or_else(
        |error| {
            prototype_error(
                diagnostics,
                prototype_type,
                prototype_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn is_parameter_recipe(prototype: &Value) -> bool {
    let Value::Object(fields) = prototype else {
        return false;
    };
    matches!(fields.get("parameter"), Some(Value::Bool(true)))
}

fn is_hidden_empty_placeholder_recipe(prototype: &Value) -> bool {
    let Value::Object(fields) = prototype else {
        return false;
    };

    matches!(fields.get("hidden"), Some(Value::Bool(true)))
        && matches!(fields.get("ingredients"), Some(Value::Object(entries)) if entries.is_empty())
        && matches!(fields.get("results"), Some(Value::Object(entries)) if entries.is_empty())
}

fn parse_machine_collection(
    collection_name: &str,
    collection: Option<Value>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Vec<Machine> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    let Value::Object(prototypes) = collection else {
        diagnostics.push(error_diagnostic(
            Some(collection_name),
            None,
            format!("/{collection_name}"),
            "prototype collection must be a JSON object",
        ));
        return Vec::new();
    };

    prototypes
        .into_iter()
        .filter_map(|(id, prototype)| {
            parse_machine(collection_name, &id, prototype, diagnostics, locale)
        })
        .collect()
}

fn parse_machine(
    prototype_type: &str,
    id: &str,
    prototype: Value,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Option<Machine> {
    let prototype_path = format!("/{prototype_type}/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some(prototype_type),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };

    let initial_errors = error_count(diagnostics);
    validate_prototype_identity(&fields, prototype_type, id, &prototype_path, diagnostics);

    let machine_id = parse_machine_id(prototype_type, id, &prototype_path, diagnostics);
    let crafting_categories =
        parse_machine_categories(&fields, prototype_type, id, &prototype_path, diagnostics);
    let crafting_speed = parse_machine_positive_number(
        &fields,
        "crafting_speed",
        prototype_type,
        id,
        &prototype_path,
        diagnostics,
    );
    let module_slots =
        parse_module_slots(&fields, prototype_type, id, &prototype_path, diagnostics);
    let allowed_effects =
        parse_allowed_effects(&fields, prototype_type, id, &prototype_path, diagnostics);
    let allowed_module_categories =
        parse_allowed_module_categories(&fields, prototype_type, id, &prototype_path, diagnostics);
    let energy_usage =
        parse_machine_energy_usage(&fields, prototype_type, id, &prototype_path, diagnostics);
    let energy_source = parse_machine_energy_source(
        &fields,
        energy_usage,
        prototype_type,
        id,
        &prototype_path,
        diagnostics,
    );

    if error_count(diagnostics) != initial_errors {
        return None;
    }

    match Machine::new(
        machine_id?,
        crafting_categories?,
        crafting_speed?,
        module_slots?,
        allowed_effects?,
        allowed_module_categories?.into_restriction(),
        energy_usage?,
        energy_source?,
    ) {
        Ok(machine) => {
            Some(machine.with_localized_name(locale_name(locale, LocalePrototypeKind::Entity, id)))
        }
        Err(error) => {
            machine_error(
                diagnostics,
                prototype_type,
                id,
                prototype_path,
                error.to_string(),
            );
            None
        }
    }
}

fn parse_machine_id(
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<MachineId> {
    MachineId::new(machine_id).map_or_else(
        |error| {
            machine_error(
                diagnostics,
                prototype_type,
                machine_id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_machine_categories(
    fields: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<BTreeSet<RecipeCategory>> {
    let path = format!("{prototype_path}/crafting_categories");
    let Some(value) = fields.get("crafting_categories") else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "missing required crafting_categories",
        );
        return None;
    };
    let Value::Array(values) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "crafting_categories must be an array",
        );
        return None;
    };
    if values.is_empty() {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "crafting_categories must contain at least one category",
        );
        return None;
    }

    let initial_errors = error_count(diagnostics);
    let categories = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            parse_category_id(
                value,
                prototype_type,
                machine_id,
                format!("{path}/{index}"),
                diagnostics,
            )
        })
        .collect();

    (error_count(diagnostics) == initial_errors).then_some(categories)
}

fn parse_category_id(
    value: &Value,
    prototype_type: &str,
    machine_id: &str,
    path: String,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RecipeCategory> {
    let Value::String(category) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "crafting category must be a string",
        );
        return None;
    };

    RecipeCategory::new(category).map_or_else(
        |error| {
            machine_error(
                diagnostics,
                prototype_type,
                machine_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_machine_positive_number(
    fields: &Map<String, Value>,
    field: &str,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let path = format!("{prototype_path}/{field}");
    let Some(value) = fields.get(field) else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            format!("missing required {field}"),
        );
        return None;
    };
    let Value::Number(number) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            format!("{field} must be a number"),
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            format!("{field} must be a finite number"),
        );
        return None;
    };

    Positive::new(value).map_or_else(
        |error| {
            machine_error(
                diagnostics,
                prototype_type,
                machine_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_module_slots(
    fields: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<u16> {
    let Some(value) = fields.get("module_slots") else {
        return Some(0);
    };
    let path = format!("{prototype_path}/module_slots");
    let Value::Number(number) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "module_slots must be a non-negative integer",
        );
        return None;
    };
    let Some(value) = number.as_u64().and_then(|value| u16::try_from(value).ok()) else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "module_slots must be an integer between 0 and 65535",
        );
        return None;
    };

    Some(value)
}

fn parse_allowed_effects(
    fields: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<BTreeSet<ModuleEffect>> {
    let Some(value) = fields.get("allowed_effects") else {
        return Some(BTreeSet::new());
    };
    let path = format!("{prototype_path}/allowed_effects");
    let initial_errors = error_count(diagnostics);
    let mut effects = BTreeSet::new();

    match value {
        Value::String(effect) => {
            if let Some(effect) =
                parse_module_effect(effect, prototype_type, machine_id, path, diagnostics)
            {
                effects.insert(effect);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let entry_path = format!("{path}/{index}");
                let Value::String(effect) = value else {
                    machine_error(
                        diagnostics,
                        prototype_type,
                        machine_id,
                        entry_path,
                        "allowed effect must be a string",
                    );
                    continue;
                };
                if let Some(effect) =
                    parse_module_effect(effect, prototype_type, machine_id, entry_path, diagnostics)
                {
                    effects.insert(effect);
                }
            }
        }
        _ => {
            machine_error(
                diagnostics,
                prototype_type,
                machine_id,
                path,
                "allowed_effects must be a string or an array",
            );
        }
    }

    (error_count(diagnostics) == initial_errors).then_some(effects)
}

fn parse_module_effect(
    effect: &str,
    prototype_type: &str,
    machine_id: &str,
    path: String,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<ModuleEffect> {
    match effect {
        "speed" => Some(ModuleEffect::Speed),
        "productivity" => Some(ModuleEffect::Productivity),
        "consumption" => Some(ModuleEffect::Consumption),
        "pollution" => Some(ModuleEffect::Pollution),
        "quality" => Some(ModuleEffect::Quality),
        _ => {
            diagnostics.push(warning_diagnostic(
                prototype_type,
                machine_id,
                path,
                format!("unsupported module effect {effect:?} was not retained"),
            ));
            None
        }
    }
}

fn parse_allowed_module_categories(
    fields: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<AllowedModuleCategories> {
    let Some(value) = fields.get("allowed_module_categories") else {
        return Some(AllowedModuleCategories::All);
    };
    let path = format!("{prototype_path}/allowed_module_categories");
    let Value::Array(values) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "allowed_module_categories must be an array",
        );
        return None;
    };

    let initial_errors = error_count(diagnostics);
    let categories = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let entry_path = format!("{path}/{index}");
            let Value::String(category) = value else {
                machine_error(
                    diagnostics,
                    prototype_type,
                    machine_id,
                    entry_path,
                    "module category must be a string",
                );
                return None;
            };
            ModuleCategory::new(category).map_or_else(
                |error| {
                    machine_error(
                        diagnostics,
                        prototype_type,
                        machine_id,
                        entry_path,
                        error.to_string(),
                    );
                    None
                },
                Some,
            )
        })
        .collect();

    (error_count(diagnostics) == initial_errors)
        .then_some(AllowedModuleCategories::Restricted(categories))
}

enum AllowedModuleCategories {
    All,
    Restricted(BTreeSet<ModuleCategory>),
}

impl AllowedModuleCategories {
    fn into_restriction(self) -> Option<BTreeSet<ModuleCategory>> {
        match self {
            Self::All => None,
            Self::Restricted(categories) => Some(categories),
        }
    }
}

fn parse_machine_energy_usage(
    fields: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let path = format!("{prototype_path}/energy_usage");
    let Some(value) = fields.get("energy_usage") else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "missing required energy_usage",
        );
        return None;
    };

    parse_energy_value(value, EnergyNormalization::Watts)
        .and_then(|watts| Positive::new(watts).map_err(|error| error.to_string()))
        .map_or_else(
            |message| {
                machine_error(
                    diagnostics,
                    prototype_type,
                    machine_id,
                    path,
                    format!("invalid energy_usage: {message}"),
                );
                None
            },
            Some,
        )
}

#[allow(clippy::too_many_arguments)]
fn parse_machine_energy_source(
    fields: &Map<String, Value>,
    energy_usage: Option<Positive>,
    prototype_type: &str,
    machine_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<MachineEnergySource> {
    let path = format!("{prototype_path}/energy_source");
    let Some(value) = fields.get("energy_source") else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "missing required energy_source",
        );
        return None;
    };
    let Value::Object(source) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "energy_source must be an object",
        );
        return None;
    };
    let type_path = format!("{path}/type");
    let Some(source_type) = source.get("type") else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            type_path,
            "missing required energy source type",
        );
        return None;
    };
    let Value::String(source_type) = source_type else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            type_path,
            "energy source type must be a string",
        );
        return None;
    };

    match source_type.as_str() {
        "electric" => parse_electric_energy_source(
            source,
            energy_usage,
            prototype_type,
            machine_id,
            &path,
            diagnostics,
        ),
        "burner" => {
            parse_burner_energy_source(source, prototype_type, machine_id, &path, diagnostics)
        }
        "heat" => Some(unsupported_energy_source(
            UnsupportedEnergySource::Heat,
            prototype_type,
            machine_id,
            type_path,
            diagnostics,
        )),
        "fluid" => Some(unsupported_energy_source(
            UnsupportedEnergySource::Fluid,
            prototype_type,
            machine_id,
            type_path,
            diagnostics,
        )),
        "void" => Some(unsupported_energy_source(
            UnsupportedEnergySource::Void,
            prototype_type,
            machine_id,
            type_path,
            diagnostics,
        )),
        unknown => Some(unsupported_energy_source(
            UnsupportedEnergySource::Unknown(unknown.to_owned()),
            prototype_type,
            machine_id,
            type_path,
            diagnostics,
        )),
    }
}

fn parse_electric_energy_source(
    source: &Map<String, Value>,
    energy_usage: Option<Positive>,
    prototype_type: &str,
    machine_id: &str,
    source_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<MachineEnergySource> {
    let drain = match source.get("drain") {
        None => NonNegative::new(energy_usage?.get() / 30.0)
            .expect("positive energy usage has a valid default drain"),
        Some(value) => {
            let path = format!("{source_path}/drain");
            let watts = parse_energy_value(value, EnergyNormalization::Watts).map_or_else(
                |message| {
                    machine_error(
                        diagnostics,
                        prototype_type,
                        machine_id,
                        path.clone(),
                        format!("invalid drain: {message}"),
                    );
                    None
                },
                Some,
            )?;
            NonNegative::new(watts).map_or_else(
                |error| {
                    machine_error(
                        diagnostics,
                        prototype_type,
                        machine_id,
                        path,
                        error.to_string(),
                    );
                    None
                },
                Some,
            )?
        }
    };

    Some(MachineEnergySource::Electric { drain })
}

fn parse_burner_energy_source(
    source: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    source_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<MachineEnergySource> {
    let initial_errors = error_count(diagnostics);
    let effectivity =
        parse_burner_effectivity(source, prototype_type, machine_id, source_path, diagnostics);
    let fuel_categories =
        parse_burner_fuel_categories(source, prototype_type, machine_id, source_path, diagnostics);

    if error_count(diagnostics) != initial_errors {
        return None;
    }

    Some(MachineEnergySource::Burner {
        fuel_categories: fuel_categories?,
        effectivity: effectivity?,
    })
}

fn parse_burner_effectivity(
    source: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    source_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let Some(value) = source.get("effectivity") else {
        return Positive::new(1.0).ok();
    };
    let path = format!("{source_path}/effectivity");
    let Value::Number(number) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "effectivity must be a number",
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "effectivity must be a finite number",
        );
        return None;
    };

    Positive::new(value).map_or_else(
        |error| {
            machine_error(
                diagnostics,
                prototype_type,
                machine_id,
                path,
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_burner_fuel_categories(
    source: &Map<String, Value>,
    prototype_type: &str,
    machine_id: &str,
    source_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<BTreeSet<FuelCategory>> {
    let Some(value) = source.get("fuel_categories") else {
        return Some(
            [FuelCategory::new(DEFAULT_BURNER_FUEL_CATEGORY)
                .expect("the default fuel category is valid")]
            .into_iter()
            .collect(),
        );
    };
    let path = format!("{source_path}/fuel_categories");
    let Value::Array(values) = value else {
        machine_error(
            diagnostics,
            prototype_type,
            machine_id,
            path,
            "fuel_categories must be an array",
        );
        return None;
    };

    let initial_errors = error_count(diagnostics);
    let categories = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let entry_path = format!("{path}/{index}");
            let Value::String(category) = value else {
                machine_error(
                    diagnostics,
                    prototype_type,
                    machine_id,
                    entry_path,
                    "fuel category must be a string",
                );
                return None;
            };
            FuelCategory::new(category).map_or_else(
                |error| {
                    machine_error(
                        diagnostics,
                        prototype_type,
                        machine_id,
                        entry_path,
                        error.to_string(),
                    );
                    None
                },
                Some,
            )
        })
        .collect();

    (error_count(diagnostics) == initial_errors).then_some(categories)
}

fn unsupported_energy_source(
    source: UnsupportedEnergySource,
    prototype_type: &str,
    machine_id: &str,
    path: String,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> MachineEnergySource {
    diagnostics.push(warning_diagnostic(
        prototype_type,
        machine_id,
        path,
        "energy source is unsupported for power and fuel calculations",
    ));
    MachineEnergySource::Unsupported(source)
}

#[derive(Clone, Copy)]
enum EnergyNormalization {
    Joules,
    Watts,
}

fn parse_energy_value(value: &Value, normalization: EnergyNormalization) -> Result<f64, String> {
    let Value::String(value) = value else {
        return Err("energy value must be a string".into());
    };
    let Some(unit) = value.chars().last() else {
        return Err("energy value must not be empty".into());
    };
    if !matches!(unit, 'W' | 'J') {
        return Err("energy value must end in W or J".into());
    }

    let mut number = &value[..value.len() - unit.len_utf8()];
    let (multiplier, has_multiplier) = match number.chars().last() {
        Some('k') => (1e3, true),
        Some('M') => (1e6, true),
        Some('G') => (1e9, true),
        Some('T') => (1e12, true),
        Some('P') => (1e15, true),
        Some('E') => (1e18, true),
        Some('Z') => (1e21, true),
        Some('Y') => (1e24, true),
        Some('R') => (1e27, true),
        Some('Q') => (1e30, true),
        _ => (1.0, false),
    };
    if has_multiplier {
        number = &number[..number.len() - 1];
    }

    let number = number
        .parse::<f64>()
        .map_err(|_| "energy value must start with a number".to_owned())?;
    if !number.is_finite() {
        return Err("energy value must be finite".into());
    }
    if number < 0.0 {
        return Err("energy value must not be negative".into());
    }

    let value = number
        * multiplier
        * match (normalization, unit) {
            (EnergyNormalization::Joules, 'J') | (EnergyNormalization::Watts, 'W') => 1.0,
            (EnergyNormalization::Joules, 'W') => 1.0 / TICKS_PER_SECOND,
            (EnergyNormalization::Watts, 'J') => TICKS_PER_SECOND,
            _ => unreachable!("energy unit was validated above"),
        };
    if !value.is_finite() {
        return Err("normalized energy value must be finite".into());
    }
    Ok(value)
}

fn parse_recipe(
    id: &str,
    prototype: Value,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
    locale: Option<&PrototypeLocale>,
) -> Option<Recipe> {
    let prototype_path = format!("/recipe/{}", pointer_segment(id));
    let Value::Object(fields) = prototype else {
        diagnostics.push(error_diagnostic(
            Some("recipe"),
            Some(id),
            prototype_path,
            "prototype must be a JSON object",
        ));
        return None;
    };

    let initial_errors = diagnostics.len();
    validate_prototype_identity(&fields, "recipe", id, &prototype_path, diagnostics);

    let recipe_id = match RecipeId::new(id) {
        Ok(id) => Some(id),
        Err(error) => {
            recipe_error(
                diagnostics,
                id,
                format!("{prototype_path}/name"),
                error.to_string(),
            );
            None
        }
    };
    let category = parse_category(&fields, id, &prototype_path, diagnostics);
    let duration = parse_duration(&fields, id, &prototype_path, diagnostics);
    let visible = parse_visibility(&fields, id, &prototype_path, diagnostics);
    let allowed_effects = parse_recipe_allowed_effects(&fields, id, &prototype_path, diagnostics);
    let allowed_module_categories =
        parse_allowed_module_categories(&fields, "recipe", id, &prototype_path, diagnostics);
    let maximum_productivity =
        parse_maximum_productivity(&fields, id, &prototype_path, diagnostics);
    let ingredients = parse_ingredients(&fields, id, &prototype_path, commodities, diagnostics);
    let products = parse_products(&fields, id, &prototype_path, commodities, diagnostics);
    let main_product = parse_main_product(
        &fields,
        id,
        &prototype_path,
        products.as_deref(),
        diagnostics,
    );

    if diagnostics.len() != initial_errors {
        return None;
    }
    let ParsedMainProduct::Valid(main_product) = main_product else {
        return None;
    };
    let allowed_effects = allowed_effects?;
    let allowed_module_categories = allowed_module_categories?.into_restriction();
    let maximum_productivity = maximum_productivity?;

    let recipe = Recipe::new(
        recipe_id?,
        category?,
        duration?,
        ingredients?,
        products?,
        main_product,
        visible?,
    )
    .map(|recipe| {
        recipe.with_module_policy(
            allowed_effects,
            allowed_module_categories,
            maximum_productivity,
        )
    });
    match recipe {
        Ok(recipe) => {
            Some(recipe.with_localized_name(locale_name(locale, LocalePrototypeKind::Recipe, id)))
        }
        Err(error) => {
            recipe_error(diagnostics, id, prototype_path, error.to_string());
            None
        }
    }
}

fn parse_recipe_allowed_effects(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<BTreeSet<ModuleEffect>> {
    let initial_errors = error_count(diagnostics);
    let mut effects = BTreeSet::new();
    for (field, effect, default) in [
        ("allow_speed", ModuleEffect::Speed, true),
        ("allow_productivity", ModuleEffect::Productivity, false),
        ("allow_consumption", ModuleEffect::Consumption, true),
    ] {
        if parse_recipe_effect_permission(
            fields,
            field,
            default,
            recipe_id,
            prototype_path,
            diagnostics,
        ) == Some(true)
        {
            effects.insert(effect);
        }
    }
    (error_count(diagnostics) == initial_errors).then_some(effects)
}

fn parse_recipe_effect_permission(
    fields: &Map<String, Value>,
    field: &str,
    default: bool,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<bool> {
    match fields.get(field) {
        None => Some(default),
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/{field}"),
                format!("{field} must be a boolean"),
            );
            None
        }
    }
}

fn parse_maximum_productivity(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<NonNegative> {
    let Some(value) = fields.get("maximum_productivity") else {
        return Some(
            NonNegative::new(DEFAULT_MAXIMUM_PRODUCTIVITY)
                .expect("Factorio's default maximum productivity is valid"),
        );
    };
    let path = format!("{prototype_path}/maximum_productivity");
    let Value::Number(number) = value else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            "maximum_productivity must be a number",
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            "maximum_productivity must be a finite number",
        );
        return None;
    };
    NonNegative::new(value).map_or_else(
        |error| {
            recipe_error(diagnostics, recipe_id, path, error.to_string());
            None
        },
        Some,
    )
}

fn validate_prototype_identity(
    fields: &Map<String, Value>,
    expected_type: &str,
    expected_name: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    validate_identity_field(
        fields,
        "type",
        expected_type,
        expected_type,
        expected_name,
        prototype_path,
        diagnostics,
    );
    validate_identity_field(
        fields,
        "name",
        expected_name,
        expected_type,
        expected_name,
        prototype_path,
        diagnostics,
    );
}

#[allow(clippy::too_many_arguments)]
fn validate_identity_field(
    fields: &Map<String, Value>,
    field: &str,
    expected: &str,
    prototype_type: &str,
    prototype_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    let path = format!("{prototype_path}/{field}");
    match fields.get(field) {
        Some(Value::String(actual)) if actual == expected => {}
        Some(Value::String(actual)) => diagnostics.push(error_diagnostic(
            Some(prototype_type),
            Some(prototype_id),
            path,
            format!("expected {field} {expected:?}, got {actual:?}"),
        )),
        Some(_) => diagnostics.push(error_diagnostic(
            Some(prototype_type),
            Some(prototype_id),
            path,
            format!("{field} must be a string"),
        )),
        None => diagnostics.push(error_diagnostic(
            Some(prototype_type),
            Some(prototype_id),
            path,
            format!("missing required {field}"),
        )),
    }
}

fn parse_category(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RecipeCategory> {
    let category = match fields.get("category") {
        None => DEFAULT_RECIPE_CATEGORY,
        Some(Value::String(category)) => category,
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/category"),
                "category must be a string",
            );
            return None;
        }
    };

    match RecipeCategory::new(category) {
        Ok(category) => Some(category),
        Err(error) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/category"),
                error.to_string(),
            );
            None
        }
    }
}

fn parse_duration(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let duration = match fields.get("energy_required") {
        None => DEFAULT_RECIPE_DURATION,
        Some(Value::Number(number)) => {
            let Some(duration) = number.as_f64() else {
                recipe_error(
                    diagnostics,
                    recipe_id,
                    format!("{prototype_path}/energy_required"),
                    "energy_required must be a finite number",
                );
                return None;
            };
            duration
        }
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/energy_required"),
                "energy_required must be a number",
            );
            return None;
        }
    };

    if duration <= MINIMUM_RECIPE_DURATION {
        recipe_error(
            diagnostics,
            recipe_id,
            format!("{prototype_path}/energy_required"),
            format!("energy_required must be greater than {MINIMUM_RECIPE_DURATION}"),
        );
        return None;
    }

    Positive::new(duration).map_or_else(
        |error| {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/energy_required"),
                error.to_string(),
            );
            None
        },
        Some,
    )
}

fn parse_visibility(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<bool> {
    match fields.get("hidden") {
        None | Some(Value::Bool(false)) => Some(true),
        Some(Value::Bool(true)) => Some(false),
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/hidden"),
                "hidden must be a boolean",
            );
            None
        }
    }
}

fn parse_ingredients(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Vec<Ingredient>> {
    let Some(value) = fields.get("ingredients") else {
        return Some(Vec::new());
    };
    let Value::Array(entries) = value else {
        recipe_error(
            diagnostics,
            recipe_id,
            format!("{prototype_path}/ingredients"),
            "ingredients must be an array",
        );
        return None;
    };

    let mut ingredients = Vec::new();
    let initial_errors = diagnostics.len();
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{prototype_path}/ingredients/{index}");
        if let Some((commodity, amount)) =
            parse_fixed_entry(entry, recipe_id, &entry_path, commodities, diagnostics)
        {
            ingredients.push(Ingredient::new(commodity, amount));
        }
    }

    (diagnostics.len() == initial_errors).then_some(ingredients)
}

fn parse_products(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Vec<Product>> {
    let Some(value) = fields.get("results") else {
        recipe_error(
            diagnostics,
            recipe_id,
            format!("{prototype_path}/results"),
            "missing required results",
        );
        return None;
    };
    let Value::Array(entries) = value else {
        recipe_error(
            diagnostics,
            recipe_id,
            format!("{prototype_path}/results"),
            "results must be an array",
        );
        return None;
    };
    if entries.is_empty() {
        recipe_error(
            diagnostics,
            recipe_id,
            format!("{prototype_path}/results"),
            "results must contain at least one product",
        );
        return None;
    }

    let mut products: Vec<AggregatedProduct> = Vec::new();
    let initial_errors = diagnostics.len();
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{prototype_path}/results/{index}");
        let entry_errors = diagnostics.len();
        let parsed = parse_product_entry(entry, recipe_id, &entry_path, commodities, diagnostics);
        if diagnostics.len() == entry_errors
            && let Some((commodity, expected_amount, productivity_amount)) = parsed
        {
            if let Some(product) = products
                .iter_mut()
                .find(|product| product.commodity == commodity)
            {
                let aggregated_amount = product.expected_amount + expected_amount;
                let aggregated_productivity = product.productivity_amount + productivity_amount;
                if aggregated_amount.is_finite() && aggregated_productivity.is_finite() {
                    product.expected_amount = aggregated_amount;
                    product.productivity_amount = aggregated_productivity;
                } else {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        entry_path,
                        "aggregated expected output and productivity amount must be finite",
                    );
                }
            } else {
                products.push(AggregatedProduct {
                    commodity,
                    expected_amount,
                    productivity_amount,
                    first_entry_path: entry_path,
                });
            }
        }
    }

    let products = products
        .into_iter()
        .filter_map(|product| {
            let amount = match Positive::new(product.expected_amount) {
                Ok(amount) => amount,
                Err(error) => {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        product.first_entry_path,
                        format!("expected output {error}"),
                    );
                    return None;
                }
            };
            let productivity_amount = NonNegative::new(product.productivity_amount)
                .expect("parsed productivity output is finite and non-negative");
            match Product::new(product.commodity, amount)
                .with_productivity_amount(productivity_amount)
            {
                Ok(product) => Some(product),
                Err(error) => {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        product.first_entry_path,
                        error.to_string(),
                    );
                    None
                }
            }
        })
        .collect();

    (diagnostics.len() == initial_errors).then_some(products)
}

struct AggregatedProduct {
    commodity: CommodityId,
    expected_amount: f64,
    productivity_amount: f64,
    first_entry_path: String,
}

fn parse_product_entry(
    entry: &Value,
    recipe_id: &str,
    entry_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<(CommodityId, f64, f64)> {
    let Value::Object(fields) = entry else {
        recipe_error(
            diagnostics,
            recipe_id,
            entry_path.into(),
            "product must be a JSON object",
        );
        return None;
    };

    let initial_errors = diagnostics.len();
    let commodity = parse_entry_commodity(fields, recipe_id, entry_path, commodities, diagnostics);
    let amount = parse_product_amount(fields, recipe_id, entry_path, diagnostics);
    let probability = parse_product_probability(fields, recipe_id, entry_path, diagnostics);
    let ignored_by_stats = parse_optional_non_negative_number(
        fields,
        "ignored_by_stats",
        recipe_id,
        entry_path,
        diagnostics,
    );
    let ignored_by_productivity = parse_optional_non_negative_number(
        fields,
        "ignored_by_productivity",
        recipe_id,
        entry_path,
        diagnostics,
    );

    if diagnostics.len() == initial_errors {
        let amount = amount?;
        let probability = probability?;
        let ignored = ignored_by_productivity
            .ok()?
            .or(ignored_by_stats.ok()?)
            .unwrap_or(0.0);
        Some((
            commodity?,
            amount * probability,
            (amount - ignored).max(0.0) * probability,
        ))
    } else {
        None
    }
}

fn parse_optional_non_negative_number(
    fields: &Map<String, Value>,
    field: &str,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<Option<f64>, ()> {
    let Some(value) = fields.get(field) else {
        return Ok(None);
    };
    let path = format!("{entry_path}/{field}");
    let Value::Number(number) = value else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be a number"),
        );
        return Err(());
    };
    let Some(value) = number.as_f64() else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be a finite number"),
        );
        return Err(());
    };
    if !value.is_finite() || value < 0.0 {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be finite and non-negative"),
        );
        return Err(());
    }
    Ok(Some(value))
}

fn parse_product_amount(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<f64> {
    if fields.contains_key("amount") {
        return parse_non_negative_number(fields, "amount", recipe_id, entry_path, diagnostics);
    }

    let amount_min =
        parse_non_negative_number(fields, "amount_min", recipe_id, entry_path, diagnostics);
    let amount_max =
        parse_non_negative_number(fields, "amount_max", recipe_id, entry_path, diagnostics);

    match (amount_min, amount_max) {
        (Some(amount_min), Some(amount_max)) => {
            let amount_max = amount_max.max(amount_min);
            Some(amount_min + (amount_max - amount_min) / 2.0)
        }
        _ => None,
    }
}

fn parse_product_probability(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<f64> {
    let Some(value) = fields.get("probability") else {
        return Some(1.0);
    };
    let path = format!("{entry_path}/probability");
    let Value::Number(number) = value else {
        recipe_error(diagnostics, recipe_id, path, "probability must be a number");
        return None;
    };
    let Some(probability) = number.as_f64() else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            "probability must be a finite number",
        );
        return None;
    };
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            "probability must be between 0 and 1",
        );
        return None;
    }

    Some(probability)
}

fn parse_non_negative_number(
    fields: &Map<String, Value>,
    field: &str,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<f64> {
    let path = format!("{entry_path}/{field}");
    let Some(value) = fields.get(field) else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("missing required {field}"),
        );
        return None;
    };
    let Value::Number(number) = value else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be a number"),
        );
        return None;
    };
    let Some(value) = number.as_f64() else {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be a finite number"),
        );
        return None;
    };
    if !value.is_finite() {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must be a finite number"),
        );
        return None;
    }
    if value < 0.0 {
        recipe_error(
            diagnostics,
            recipe_id,
            path,
            format!("{field} must not be negative"),
        );
        return None;
    }

    Some(value)
}

fn parse_fixed_entry(
    entry: &Value,
    recipe_id: &str,
    entry_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<(CommodityId, Positive)> {
    let Value::Object(fields) = entry else {
        recipe_error(
            diagnostics,
            recipe_id,
            entry_path.into(),
            "ingredient or product must be a JSON object",
        );
        return None;
    };

    let initial_errors = diagnostics.len();
    let commodity = parse_entry_commodity(fields, recipe_id, entry_path, commodities, diagnostics);
    let amount = parse_entry_amount(fields, recipe_id, entry_path, diagnostics);

    if diagnostics.len() == initial_errors {
        Some((commodity?, amount?))
    } else {
        None
    }
}

fn parse_entry_commodity(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<CommodityId> {
    let kind = parse_entry_type(fields, recipe_id, entry_path, diagnostics);
    let name = parse_entry_name(fields, recipe_id, entry_path, diagnostics);

    match (kind, name) {
        (Some(kind), Some(name)) => match kind.id(name.clone()) {
            Ok(commodity) => {
                if !commodities.contains(&commodity) {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        format!("{entry_path}/name"),
                        format!("references missing {} {name:?}", kind.prototype_type()),
                    );
                }
                Some(commodity)
            }
            Err(error) => {
                recipe_error(
                    diagnostics,
                    recipe_id,
                    format!("{entry_path}/name"),
                    error.to_string(),
                );
                None
            }
        },
        _ => None,
    }
}

fn parse_entry_type(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<CommodityKind> {
    match fields.get("type") {
        Some(Value::String(value)) if value == "item" => Some(CommodityKind::Item),
        Some(Value::String(value)) if value == "fluid" => Some(CommodityKind::Fluid),
        Some(Value::String(value)) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/type"),
                format!("unsupported commodity type {value:?}"),
            );
            None
        }
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/type"),
                "type must be a string",
            );
            None
        }
        None => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/type"),
                "missing required type",
            );
            None
        }
    }
}

fn parse_entry_name(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<String> {
    match fields.get("name") {
        Some(Value::String(name)) => Some(name.clone()),
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/name"),
                "name must be a string",
            );
            None
        }
        None => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/name"),
                "missing required name",
            );
            None
        }
    }
}

fn parse_entry_amount(
    fields: &Map<String, Value>,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Positive> {
    let amount = match fields.get("amount") {
        Some(Value::Number(number)) => {
            let Some(amount) = number.as_f64() else {
                recipe_error(
                    diagnostics,
                    recipe_id,
                    format!("{entry_path}/amount"),
                    "amount must be a finite number",
                );
                return None;
            };
            amount
        }
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/amount"),
                "amount must be a number",
            );
            return None;
        }
        None => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/amount"),
                "missing required fixed amount",
            );
            return None;
        }
    };

    Positive::new(amount).map_or_else(
        |error| {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/amount"),
                error.to_string(),
            );
            None
        },
        Some,
    )
}

enum ParsedMainProduct {
    Valid(Option<CommodityId>),
    Invalid,
}

fn parse_main_product(
    fields: &Map<String, Value>,
    recipe_id: &str,
    prototype_path: &str,
    products: Option<&[Product]>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> ParsedMainProduct {
    let Some(products) = products else {
        return ParsedMainProduct::Invalid;
    };
    let distinct_products = products
        .iter()
        .map(|product| product.commodity().clone())
        .collect::<BTreeSet<_>>();

    match fields.get("main_product") {
        None => ParsedMainProduct::Valid((distinct_products.len() == 1).then(|| {
            distinct_products
                .iter()
                .next()
                .expect("one distinct product exists")
                .clone()
        })),
        Some(Value::String(main_product)) if main_product.is_empty() => {
            ParsedMainProduct::Valid(None)
        }
        Some(Value::String(main_product)) => {
            let matches = distinct_products
                .iter()
                .filter(|commodity| commodity_name(commodity) == main_product)
                .cloned()
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [commodity] => ParsedMainProduct::Valid(Some(commodity.clone())),
                [] => {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        format!("{prototype_path}/main_product"),
                        format!("main product {main_product:?} is not produced by the recipe"),
                    );
                    ParsedMainProduct::Invalid
                }
                _ => {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        format!("{prototype_path}/main_product"),
                        format!("main product {main_product:?} is ambiguous"),
                    );
                    ParsedMainProduct::Invalid
                }
            }
        }
        Some(_) => {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{prototype_path}/main_product"),
                "main_product must be a string",
            );
            ParsedMainProduct::Invalid
        }
    }
}

fn commodity_name(commodity: &CommodityId) -> &str {
    match commodity {
        CommodityId::Item(id) => id.as_str(),
        CommodityId::Fluid(id) => id.as_str(),
    }
}

fn recipe_error(
    diagnostics: &mut Vec<ImportDiagnostic>,
    recipe_id: &str,
    path: String,
    message: impl Into<String>,
) {
    diagnostics.push(error_diagnostic(
        Some("recipe"),
        Some(recipe_id),
        path,
        message,
    ));
}

fn machine_error(
    diagnostics: &mut Vec<ImportDiagnostic>,
    prototype_type: &str,
    machine_id: &str,
    path: String,
    message: impl Into<String>,
) {
    diagnostics.push(error_diagnostic(
        Some(prototype_type),
        Some(machine_id),
        path,
        message,
    ));
}

fn prototype_error(
    diagnostics: &mut Vec<ImportDiagnostic>,
    prototype_type: &str,
    prototype_id: &str,
    path: String,
    message: impl Into<String>,
) {
    diagnostics.push(error_diagnostic(
        Some(prototype_type),
        Some(prototype_id),
        path,
        message,
    ));
}

fn error_diagnostic(
    prototype_type: Option<&str>,
    prototype_id: Option<&str>,
    path: String,
    message: impl Into<String>,
) -> ImportDiagnostic {
    ImportDiagnostic {
        severity: DiagnosticSeverity::Error,
        prototype_type: prototype_type.map(str::to_owned),
        prototype_id: prototype_id.map(str::to_owned),
        path,
        message: message.into(),
        disposition: PrototypeDisposition::Rejected,
    }
}

fn warning_diagnostic(
    prototype_type: &str,
    prototype_id: &str,
    path: String,
    message: impl Into<String>,
) -> ImportDiagnostic {
    ImportDiagnostic {
        severity: DiagnosticSeverity::Warning,
        prototype_type: Some(prototype_type.to_owned()),
        prototype_id: Some(prototype_id.to_owned()),
        path,
        message: message.into(),
        disposition: PrototypeDisposition::PartiallyRetained,
    }
}

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn locale_name(
    locale: Option<&PrototypeLocale>,
    kind: LocalePrototypeKind,
    id: &str,
) -> Option<String> {
    locale
        .and_then(|locale| locale.localized_name(kind, id))
        .map(str::to_owned)
}

fn error_count(diagnostics: &[ImportDiagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count()
}

fn has_errors(diagnostics: &[ImportDiagnostic]) -> bool {
    error_count(diagnostics) > 0
}
