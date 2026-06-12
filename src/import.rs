use std::collections::BTreeSet;
use std::io::Read;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::catalog::{
    Catalog, CatalogParts, Commodity, CommodityId, FluidId, Ingredient, ItemId, Positive, Product,
    Recipe, RecipeCategory, RecipeId,
};

const DEFAULT_RECIPE_CATEGORY: &str = "crafting";
const DEFAULT_RECIPE_DURATION: f64 = 0.5;
const MINIMUM_RECIPE_DURATION: f64 = 0.001;

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

    if has_errors(&diagnostics) {
        return Err(ImportError::InvalidData { diagnostics });
    }

    let catalog = Catalog::try_from_parts(CatalogParts {
        commodities,
        recipes,
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

    let mut products = Vec::new();
    let initial_errors = diagnostics.len();
    for (index, entry) in entries.iter().enumerate() {
        let entry_path = format!("{prototype_path}/results/{index}");
        let entry_errors = diagnostics.len();
        let parsed = parse_fixed_entry(entry, recipe_id, &entry_path, commodities, diagnostics);
        reject_non_fixed_product_fields(entry, recipe_id, &entry_path, diagnostics);
        if diagnostics.len() == entry_errors
            && let Some((commodity, amount)) = parsed
        {
            products.push(Product::new(commodity, amount));
        }
    }

    (diagnostics.len() == initial_errors).then_some(products)
}

fn reject_non_fixed_product_fields(
    entry: &Value,
    recipe_id: &str,
    entry_path: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    let Value::Object(fields) = entry else {
        return;
    };
    for field in ["amount_min", "amount_max"] {
        if fields.contains_key(field) {
            recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/{field}"),
                "ranged products are not supported by this importer milestone",
            );
        }
    }
    if let Some(probability) = fields.get("probability") {
        match probability {
            Value::Number(number) if number.as_f64() == Some(1.0) => {}
            Value::Number(_) => recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/probability"),
                "probabilistic products are not supported by this importer milestone",
            ),
            _ => recipe_error(
                diagnostics,
                recipe_id,
                format!("{entry_path}/probability"),
                "probability must be a number",
            ),
        }
    }
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
    let kind = parse_entry_type(fields, recipe_id, entry_path, diagnostics);
    let name = parse_entry_name(fields, recipe_id, entry_path, diagnostics);
    let amount = parse_entry_amount(fields, recipe_id, entry_path, diagnostics);

    let commodity = match (kind, name) {
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
    };

    if diagnostics.len() == initial_errors {
        Some((commodity?, amount?))
    } else {
        None
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

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn has_errors(diagnostics: &[ImportDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}
