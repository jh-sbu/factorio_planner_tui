use std::collections::BTreeSet;
use std::io::Read;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::catalog::{
    Catalog, CatalogParts, Commodity, CommodityId, FluidId, FuelCategory, Ingredient, ItemId,
    Machine, MachineEnergySource, MachineId, ModuleCategory, ModuleEffect, NonNegative, Positive,
    Product, Recipe, RecipeCategory, RecipeId, UnsupportedEnergySource,
};

const DEFAULT_RECIPE_CATEGORY: &str = "crafting";
const DEFAULT_RECIPE_DURATION: f64 = 0.5;
const MINIMUM_RECIPE_DURATION: f64 = 0.001;
const DEFAULT_BURNER_FUEL_CATEGORY: &str = "chemical";
const TICKS_PER_SECOND: f64 = 60.0;

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

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ImportError {
    #[error("invalid JSON at line {line}, column {column}: {message}")]
    Json {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("data.raw contains invalid supported prototype data")]
    InvalidData { diagnostics: Vec<ImportDiagnostic> },
}

#[derive(Debug, Default, Deserialize)]
struct RelevantCollections {
    item: Option<Value>,
    fluid: Option<Value>,
    recipe: Option<Value>,
    #[serde(rename = "assembling-machine")]
    assembling_machine: Option<Value>,
    furnace: Option<Value>,
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
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let raw =
        RelevantCollections::deserialize(&mut deserializer).map_err(|error| json_error(&error))?;
    deserializer.end().map_err(|error| json_error(&error))?;

    let mut diagnostics = Vec::new();
    let mut commodities = Vec::new();
    parse_commodity_collection(
        "item",
        raw.item,
        CommodityKind::Item,
        &mut commodities,
        &mut diagnostics,
    );
    parse_commodity_collection(
        "fluid",
        raw.fluid,
        CommodityKind::Fluid,
        &mut commodities,
        &mut diagnostics,
    );

    let commodity_ids = commodities
        .iter()
        .map(|commodity| commodity.id().clone())
        .collect::<BTreeSet<_>>();
    let recipes = parse_recipe_collection(raw.recipe, &commodity_ids, &mut diagnostics);
    let mut machines = parse_machine_collection(
        "assembling-machine",
        raw.assembling_machine,
        &mut diagnostics,
    );
    machines.extend(parse_machine_collection(
        "furnace",
        raw.furnace,
        &mut diagnostics,
    ));

    if has_errors(&diagnostics) {
        return Err(ImportError::InvalidData { diagnostics });
    }

    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities,
        recipes,
        machines,
        ..CatalogParts::default()
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

fn parse_commodity_collection(
    collection_name: &str,
    collection: Option<Value>,
    kind: CommodityKind,
    commodities: &mut Vec<Commodity>,
    diagnostics: &mut Vec<ImportDiagnostic>,
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

        let commodity_id = match kind.id(id.clone()) {
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
            && let Some(commodity_id) = commodity_id
        {
            commodities.push(Commodity::new(commodity_id, None));
        }
    }
}

fn parse_recipe_collection(
    collection: Option<Value>,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
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
        .filter_map(|(id, prototype)| parse_recipe(&id, prototype, commodities, diagnostics))
        .collect()
}

fn parse_machine_collection(
    collection_name: &str,
    collection: Option<Value>,
    diagnostics: &mut Vec<ImportDiagnostic>,
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
        .filter_map(|(id, prototype)| parse_machine(collection_name, &id, prototype, diagnostics))
        .collect()
}

fn parse_machine(
    prototype_type: &str,
    id: &str,
    prototype: Value,
    diagnostics: &mut Vec<ImportDiagnostic>,
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
        Ok(machine) => Some(machine),
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

    parse_energy_value(value)
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
            let watts = parse_energy_value(value).map_or_else(
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

fn parse_energy_value(value: &Value) -> Result<f64, String> {
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

    let watts = number * multiplier * if unit == 'J' { TICKS_PER_SECOND } else { 1.0 };
    if !watts.is_finite() {
        return Err("normalized energy value must be finite".into());
    }
    Ok(watts)
}

fn parse_recipe(
    id: &str,
    prototype: Value,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
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

    let recipe = Recipe::new(
        recipe_id?,
        category?,
        duration?,
        ingredients?,
        products?,
        main_product,
        visible?,
    );
    match recipe {
        Ok(recipe) => Some(recipe),
        Err(error) => {
            recipe_error(diagnostics, id, prototype_path, error.to_string());
            None
        }
    }
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
            && let Some((commodity, expected_amount)) = parsed
        {
            if let Some(product) = products
                .iter_mut()
                .find(|product| product.commodity == commodity)
            {
                let aggregated_amount = product.expected_amount + expected_amount;
                if aggregated_amount.is_finite() {
                    product.expected_amount = aggregated_amount;
                } else {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        entry_path,
                        "aggregated expected output must be finite",
                    );
                }
            } else {
                products.push(AggregatedProduct {
                    commodity,
                    expected_amount,
                    first_entry_path: entry_path,
                });
            }
        }
    }

    let products = products
        .into_iter()
        .filter_map(|product| {
            Positive::new(product.expected_amount).map_or_else(
                |error| {
                    recipe_error(
                        diagnostics,
                        recipe_id,
                        product.first_entry_path,
                        format!("expected output {error}"),
                    );
                    None
                },
                |amount| Some(Product::new(product.commodity, amount)),
            )
        })
        .collect();

    (diagnostics.len() == initial_errors).then_some(products)
}

struct AggregatedProduct {
    commodity: CommodityId,
    expected_amount: f64,
    first_entry_path: String,
}

fn parse_product_entry(
    entry: &Value,
    recipe_id: &str,
    entry_path: &str,
    commodities: &BTreeSet<CommodityId>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<(CommodityId, f64)> {
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

    if diagnostics.len() == initial_errors {
        Some((commodity?, amount? * probability?))
    } else {
        None
    }
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

fn error_count(diagnostics: &[ImportDiagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count()
}

fn has_errors(diagnostics: &[ImportDiagnostic]) -> bool {
    error_count(diagnostics) > 0
}
