use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier while preserving its authoritative prototype text.
            ///
            /// # Errors
            ///
            /// Returns [`IdentifierError::Empty`] when `value` is empty.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(IdentifierError::Empty);
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum IdentifierError {
    #[error("identifier must not be empty")]
    Empty,
}

string_id!(ItemId);
string_id!(FluidId);
string_id!(RecipeId);
string_id!(MachineId);
string_id!(ModuleId);
string_id!(FuelId);
string_id!(BeltId);
string_id!(DatasetFingerprint);
string_id!(RecipeCategory);
string_id!(ModuleCategory);
string_id!(FuelCategory);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommodityId {
    Item(ItemId),
    Fluid(FluidId),
}

impl From<ItemId> for CommodityId {
    fn from(value: ItemId) -> Self {
        Self::Item(value)
    }
}

impl From<FluidId> for CommodityId {
    fn from(value: FluidId) -> Self {
        Self::Fluid(value)
    }
}

impl fmt::Display for CommodityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Item(id) => id.fmt(formatter),
            Self::Fluid(id) => id.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum NumericError {
    #[error("value must be finite, got {0}")]
    NotFinite(f64),
    #[error("value must be greater than zero, got {0}")]
    NotPositive(f64),
    #[error("value must not be negative, got {0}")]
    Negative(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Positive(f64);

impl Positive {
    /// Creates a finite value greater than zero.
    ///
    /// # Errors
    ///
    /// Returns [`NumericError`] when `value` is non-finite or not positive.
    pub fn new(value: f64) -> Result<Self, NumericError> {
        Finite::new(value)?;
        if value <= 0.0 {
            return Err(NumericError::NotPositive(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NonNegative(f64);

impl NonNegative {
    /// Creates a finite value greater than or equal to zero.
    ///
    /// # Errors
    ///
    /// Returns [`NumericError`] when `value` is non-finite or negative.
    pub fn new(value: f64) -> Result<Self, NumericError> {
        Finite::new(value)?;
        if value < 0.0 {
            return Err(NumericError::Negative(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finite(f64);

impl Finite {
    /// Creates a finite floating-point value.
    ///
    /// # Errors
    ///
    /// Returns [`NumericError::NotFinite`] when `value` is infinite or NaN.
    pub fn new(value: f64) -> Result<Self, NumericError> {
        if !value.is_finite() {
            return Err(NumericError::NotFinite(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commodity {
    id: CommodityId,
    display_name: Option<String>,
}

impl Commodity {
    #[must_use]
    pub const fn new(id: CommodityId, display_name: Option<String>) -> Self {
        Self { id, display_name }
    }

    #[must_use]
    pub const fn id(&self) -> &CommodityId {
        &self.id
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ingredient {
    commodity: CommodityId,
    amount: Positive,
}

impl Ingredient {
    #[must_use]
    pub const fn new(commodity: CommodityId, amount: Positive) -> Self {
        Self { commodity, amount }
    }

    #[must_use]
    pub const fn commodity(&self) -> &CommodityId {
        &self.commodity
    }

    #[must_use]
    pub const fn amount(&self) -> Positive {
        self.amount
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    commodity: CommodityId,
    amount: Positive,
}

impl Product {
    #[must_use]
    pub const fn new(commodity: CommodityId, amount: Positive) -> Self {
        Self { commodity, amount }
    }

    #[must_use]
    pub const fn commodity(&self) -> &CommodityId {
        &self.commodity
    }

    #[must_use]
    pub const fn amount(&self) -> Positive {
        self.amount
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    id: RecipeId,
    category: RecipeCategory,
    duration: Positive,
    ingredients: Vec<Ingredient>,
    products: Vec<Product>,
    main_product: Option<CommodityId>,
    visible: bool,
}

impl Recipe {
    /// Creates a normalized recipe.
    ///
    /// # Errors
    ///
    /// Returns [`RecordError`] when the recipe has no products or its declared
    /// main product is not among its products.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RecipeId,
        category: RecipeCategory,
        duration: Positive,
        ingredients: Vec<Ingredient>,
        products: Vec<Product>,
        main_product: Option<CommodityId>,
        visible: bool,
    ) -> Result<Self, RecordError> {
        if products.is_empty() {
            return Err(RecordError::RecipeHasNoProducts { recipe: id });
        }
        if let Some(main_product) = &main_product
            && !products
                .iter()
                .any(|product| product.commodity() == main_product)
        {
            return Err(RecordError::MainProductNotProduced {
                recipe: id,
                commodity: main_product.clone(),
            });
        }
        Ok(Self {
            id,
            category,
            duration,
            ingredients,
            products,
            main_product,
            visible,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &RecipeId {
        &self.id
    }

    #[must_use]
    pub const fn category(&self) -> &RecipeCategory {
        &self.category
    }

    #[must_use]
    pub const fn duration(&self) -> Positive {
        self.duration
    }

    #[must_use]
    pub fn ingredients(&self) -> &[Ingredient] {
        &self.ingredients
    }

    #[must_use]
    pub fn products(&self) -> &[Product] {
        &self.products
    }

    #[must_use]
    pub const fn main_product(&self) -> Option<&CommodityId> {
        self.main_product.as_ref()
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    id: MachineId,
    crafting_categories: BTreeSet<RecipeCategory>,
    crafting_speed: Positive,
}

impl Machine {
    /// Creates a crafting machine.
    ///
    /// # Errors
    ///
    /// Returns [`RecordError::MachineHasNoCraftingCategories`] when no
    /// crafting category is supplied.
    pub fn new(
        id: MachineId,
        crafting_categories: impl IntoIterator<Item = RecipeCategory>,
        crafting_speed: Positive,
    ) -> Result<Self, RecordError> {
        let crafting_categories = crafting_categories.into_iter().collect::<BTreeSet<_>>();
        if crafting_categories.is_empty() {
            return Err(RecordError::MachineHasNoCraftingCategories { machine: id });
        }
        Ok(Self {
            id,
            crafting_categories,
            crafting_speed,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &MachineId {
        &self.id
    }

    #[must_use]
    pub const fn crafting_categories(&self) -> &BTreeSet<RecipeCategory> {
        &self.crafting_categories
    }

    #[must_use]
    pub const fn crafting_speed(&self) -> Positive {
        self.crafting_speed
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    id: ModuleId,
    category: ModuleCategory,
    speed_effect: Finite,
    productivity_effect: Finite,
    consumption_effect: Finite,
}

impl Module {
    #[must_use]
    pub const fn new(
        id: ModuleId,
        category: ModuleCategory,
        speed_effect: Finite,
        productivity_effect: Finite,
        consumption_effect: Finite,
    ) -> Self {
        Self {
            id,
            category,
            speed_effect,
            productivity_effect,
            consumption_effect,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ModuleId {
        &self.id
    }

    #[must_use]
    pub const fn category(&self) -> &ModuleCategory {
        &self.category
    }

    #[must_use]
    pub const fn speed_effect(&self) -> Finite {
        self.speed_effect
    }

    #[must_use]
    pub const fn productivity_effect(&self) -> Finite {
        self.productivity_effect
    }

    #[must_use]
    pub const fn consumption_effect(&self) -> Finite {
        self.consumption_effect
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fuel {
    id: FuelId,
    item: ItemId,
    category: FuelCategory,
    value: Positive,
    burnt_result: Option<ItemId>,
}

impl Fuel {
    #[must_use]
    pub const fn new(
        id: FuelId,
        item: ItemId,
        category: FuelCategory,
        fuel_value: Positive,
        burnt_result: Option<ItemId>,
    ) -> Self {
        Self {
            id,
            item,
            category,
            value: fuel_value,
            burnt_result,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &FuelId {
        &self.id
    }

    #[must_use]
    pub const fn item(&self) -> &ItemId {
        &self.item
    }

    #[must_use]
    pub const fn category(&self) -> &FuelCategory {
        &self.category
    }

    #[must_use]
    pub const fn fuel_value(&self) -> Positive {
        self.value
    }

    #[must_use]
    pub const fn burnt_result(&self) -> Option<&ItemId> {
        self.burnt_result.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Belt {
    id: BeltId,
    throughput: Positive,
}

impl Belt {
    #[must_use]
    pub const fn new(id: BeltId, throughput: Positive) -> Self {
        Self { id, throughput }
    }

    #[must_use]
    pub const fn id(&self) -> &BeltId {
        &self.id
    }

    #[must_use]
    pub const fn throughput(&self) -> Positive {
        self.throughput
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum RecordError {
    #[error("recipe {recipe} has no products")]
    RecipeHasNoProducts { recipe: RecipeId },
    #[error("recipe {recipe} does not produce its main product {commodity}")]
    MainProductNotProduced {
        recipe: RecipeId,
        commodity: CommodityId,
    },
    #[error("machine {machine} has no crafting categories")]
    MachineHasNoCraftingCategories { machine: MachineId },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CatalogParts {
    pub commodities: Vec<Commodity>,
    pub recipes: Vec<Recipe>,
    pub machines: Vec<Machine>,
    pub modules: Vec<Module>,
    pub fuels: Vec<Fuel>,
    pub belts: Vec<Belt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    commodities: BTreeMap<CommodityId, Commodity>,
    recipes: BTreeMap<RecipeId, Recipe>,
    machines: BTreeMap<MachineId, Machine>,
    modules: BTreeMap<ModuleId, Module>,
    fuels: BTreeMap<FuelId, Fuel>,
    belts: BTreeMap<BeltId, Belt>,
    recipes_by_product: BTreeMap<CommodityId, Vec<RecipeId>>,
    machines_by_category: BTreeMap<RecipeCategory, Vec<MachineId>>,
}

impl Catalog {
    /// Builds a catalog and all deterministic reverse indexes.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for duplicate IDs or references to commodities
    /// that are not present in `parts`.
    pub fn try_from_parts(parts: CatalogParts) -> Result<Self, CatalogError> {
        let commodities = collect_unique(parts.commodities, Commodity::id, |id| {
            CatalogError::DuplicateCommodity { id }
        })?;
        let recipes = collect_unique(parts.recipes, Recipe::id, |id| {
            CatalogError::DuplicateRecipe { id }
        })?;
        let machines = collect_unique(parts.machines, Machine::id, |id| {
            CatalogError::DuplicateMachine { id }
        })?;
        let modules = collect_unique(parts.modules, Module::id, |id| {
            CatalogError::DuplicateModule { id }
        })?;
        let fuels = collect_unique(parts.fuels, Fuel::id, |id| CatalogError::DuplicateFuel {
            id,
        })?;
        let belts = collect_unique(parts.belts, Belt::id, |id| CatalogError::DuplicateBelt {
            id,
        })?;

        validate_references(&commodities, &recipes, &fuels)?;

        let mut recipes_by_product: BTreeMap<CommodityId, Vec<RecipeId>> = BTreeMap::new();
        for (recipe_id, recipe) in &recipes {
            for product in recipe.products() {
                let recipe_ids = recipes_by_product
                    .entry(product.commodity().clone())
                    .or_default();
                if !recipe_ids.contains(recipe_id) {
                    recipe_ids.push(recipe_id.clone());
                }
            }
        }

        let mut machines_by_category: BTreeMap<RecipeCategory, Vec<MachineId>> = BTreeMap::new();
        for (machine_id, machine) in &machines {
            for category in machine.crafting_categories() {
                machines_by_category
                    .entry(category.clone())
                    .or_default()
                    .push(machine_id.clone());
            }
        }

        Ok(Self {
            commodities,
            recipes,
            machines,
            modules,
            fuels,
            belts,
            recipes_by_product,
            machines_by_category,
        })
    }

    #[must_use]
    pub fn commodity(&self, id: &CommodityId) -> Option<&Commodity> {
        self.commodities.get(id)
    }

    #[must_use]
    pub fn recipe(&self, id: &RecipeId) -> Option<&Recipe> {
        self.recipes.get(id)
    }

    #[must_use]
    pub fn machine(&self, id: &MachineId) -> Option<&Machine> {
        self.machines.get(id)
    }

    #[must_use]
    pub fn module(&self, id: &ModuleId) -> Option<&Module> {
        self.modules.get(id)
    }

    #[must_use]
    pub fn fuel(&self, id: &FuelId) -> Option<&Fuel> {
        self.fuels.get(id)
    }

    #[must_use]
    pub fn belt(&self, id: &BeltId) -> Option<&Belt> {
        self.belts.get(id)
    }

    #[must_use]
    pub fn recipes_for_product(&self, id: &CommodityId) -> &[RecipeId] {
        self.recipes_by_product.get(id).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn machines_for_category(&self, category: &RecipeCategory) -> &[MachineId] {
        self.machines_by_category
            .get(category)
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn commodities(&self) -> impl ExactSizeIterator<Item = &Commodity> {
        self.commodities.values()
    }

    #[must_use]
    pub fn recipes(&self) -> impl ExactSizeIterator<Item = &Recipe> {
        self.recipes.values()
    }

    #[must_use]
    pub fn machines(&self) -> impl ExactSizeIterator<Item = &Machine> {
        self.machines.values()
    }

    #[must_use]
    pub fn modules(&self) -> impl ExactSizeIterator<Item = &Module> {
        self.modules.values()
    }

    #[must_use]
    pub fn fuels(&self) -> impl ExactSizeIterator<Item = &Fuel> {
        self.fuels.values()
    }

    #[must_use]
    pub fn belts(&self) -> impl ExactSizeIterator<Item = &Belt> {
        self.belts.values()
    }
}

fn collect_unique<T, Id, E>(
    values: Vec<T>,
    id: impl Fn(&T) -> &Id,
    duplicate_error: impl Fn(Id) -> E,
) -> Result<BTreeMap<Id, T>, E>
where
    Id: Clone + Ord,
{
    let mut records = BTreeMap::new();
    for value in values {
        let id = id(&value).clone();
        if records.insert(id.clone(), value).is_some() {
            return Err(duplicate_error(id));
        }
    }
    Ok(records)
}

fn validate_references(
    commodities: &BTreeMap<CommodityId, Commodity>,
    recipes: &BTreeMap<RecipeId, Recipe>,
    fuels: &BTreeMap<FuelId, Fuel>,
) -> Result<(), CatalogError> {
    for (recipe_id, recipe) in recipes {
        for ingredient in recipe.ingredients() {
            if !commodities.contains_key(ingredient.commodity()) {
                return Err(CatalogError::MissingRecipeIngredient {
                    recipe: recipe_id.clone(),
                    commodity: ingredient.commodity().clone(),
                });
            }
        }
        for product in recipe.products() {
            if !commodities.contains_key(product.commodity()) {
                return Err(CatalogError::MissingRecipeProduct {
                    recipe: recipe_id.clone(),
                    commodity: product.commodity().clone(),
                });
            }
        }
    }

    for (fuel_id, fuel) in fuels {
        if !commodities.contains_key(&CommodityId::Item(fuel.item().clone())) {
            return Err(CatalogError::MissingFuelItem {
                fuel: fuel_id.clone(),
                item: fuel.item().clone(),
            });
        }
        if let Some(burnt_result) = fuel.burnt_result()
            && !commodities.contains_key(&CommodityId::Item(burnt_result.clone()))
        {
            return Err(CatalogError::MissingFuelBurntResult {
                fuel: fuel_id.clone(),
                item: burnt_result.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum CatalogError {
    #[error("duplicate commodity ID {id}")]
    DuplicateCommodity { id: CommodityId },
    #[error("duplicate recipe ID {id}")]
    DuplicateRecipe { id: RecipeId },
    #[error("duplicate machine ID {id}")]
    DuplicateMachine { id: MachineId },
    #[error("duplicate module ID {id}")]
    DuplicateModule { id: ModuleId },
    #[error("duplicate fuel ID {id}")]
    DuplicateFuel { id: FuelId },
    #[error("duplicate belt ID {id}")]
    DuplicateBelt { id: BeltId },
    #[error("recipe {recipe} references missing ingredient {commodity}")]
    MissingRecipeIngredient {
        recipe: RecipeId,
        commodity: CommodityId,
    },
    #[error("recipe {recipe} references missing product {commodity}")]
    MissingRecipeProduct {
        recipe: RecipeId,
        commodity: CommodityId,
    },
    #[error("fuel {fuel} references missing item {item}")]
    MissingFuelItem { fuel: FuelId, item: ItemId },
    #[error("fuel {fuel} references missing burnt-result item {item}")]
    MissingFuelBurntResult { fuel: FuelId, item: ItemId },
}
