use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::catalog::{
    Catalog, CommodityId, MachineId, NumericError, Positive, Recipe, RecipeCategory, RecipeId,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RateUnit {
    #[default]
    Second,
    Minute,
    Hour,
}

impl RateUnit {
    #[must_use]
    pub fn convert_rate(self, rate_per_second: Positive) -> f64 {
        let multiplier = match self {
            Self::Second => 1.0,
            Self::Minute => 60.0,
            Self::Hour => 3_600.0,
        };
        rate_per_second.get() * multiplier
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    commodity: CommodityId,
    rate_per_second: Positive,
}

impl Target {
    /// Creates a production target expressed in units per second.
    ///
    /// # Errors
    ///
    /// Returns [`NumericError`] when `rate_per_second` is non-finite or not
    /// positive.
    pub fn new(commodity: CommodityId, rate_per_second: f64) -> Result<Self, NumericError> {
        Ok(Self {
            commodity,
            rate_per_second: Positive::new(rate_per_second)?,
        })
    }

    #[must_use]
    pub const fn commodity(&self) -> &CommodityId {
        &self.commodity
    }

    #[must_use]
    pub const fn rate_per_second(&self) -> Positive {
        self.rate_per_second
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FactoryPlan {
    target: Target,
    external_inputs: BTreeSet<CommodityId>,
    display_rate_unit: RateUnit,
}

impl FactoryPlan {
    #[must_use]
    pub fn new(target: Target) -> Self {
        Self {
            target,
            external_inputs: BTreeSet::new(),
            display_rate_unit: RateUnit::default(),
        }
    }

    #[must_use]
    pub fn with_external_inputs(
        mut self,
        external_inputs: impl IntoIterator<Item = CommodityId>,
    ) -> Self {
        self.external_inputs = external_inputs.into_iter().collect();
        self
    }

    #[must_use]
    pub const fn with_display_rate_unit(mut self, display_rate_unit: RateUnit) -> Self {
        self.display_rate_unit = display_rate_unit;
        self
    }

    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }

    #[must_use]
    pub const fn external_inputs(&self) -> &BTreeSet<CommodityId> {
        &self.external_inputs
    }

    #[must_use]
    pub const fn display_rate_unit(&self) -> RateUnit {
        self.display_rate_unit
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommodityRate {
    commodity: CommodityId,
    rate: Positive,
}

impl CommodityRate {
    #[must_use]
    pub const fn commodity(&self) -> &CommodityId {
        &self.commodity
    }

    #[must_use]
    pub const fn rate(&self) -> Positive {
        self.rate
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProductionStep {
    planning_product: CommodityId,
    recipe: RecipeId,
    machine: MachineId,
    required_output_rate: Positive,
    craft_rate: Positive,
    fractional_machine_count: Positive,
    installed_machine_count: u64,
    ingredients: Vec<CommodityRate>,
}

impl ProductionStep {
    #[must_use]
    pub const fn planning_product(&self) -> &CommodityId {
        &self.planning_product
    }

    #[must_use]
    pub const fn recipe(&self) -> &RecipeId {
        &self.recipe
    }

    #[must_use]
    pub const fn machine(&self) -> &MachineId {
        &self.machine
    }

    #[must_use]
    pub const fn required_output_rate(&self) -> Positive {
        self.required_output_rate
    }

    #[must_use]
    pub const fn craft_rate(&self) -> Positive {
        self.craft_rate
    }

    #[must_use]
    pub const fn fractional_machine_count(&self) -> Positive {
        self.fractional_machine_count
    }

    #[must_use]
    pub const fn installed_machine_count(&self) -> u64 {
        self.installed_machine_count
    }

    #[must_use]
    pub fn ingredients(&self) -> &[CommodityRate] {
        &self.ingredients
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalculationResult {
    production_steps: Vec<ProductionStep>,
    external_inputs: Vec<CommodityRate>,
    display_rate_unit: RateUnit,
}

impl CalculationResult {
    #[must_use]
    pub fn production_steps(&self) -> &[ProductionStep] {
        &self.production_steps
    }

    #[must_use]
    pub fn external_inputs(&self) -> &[CommodityRate] {
        &self.external_inputs
    }

    #[must_use]
    pub const fn display_rate_unit(&self) -> RateUnit {
        self.display_rate_unit
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum PlannerError {
    #[error("target commodity {commodity} is not present in the catalog")]
    UnknownTarget { commodity: CommodityId },
    #[error("commodity {commodity} has multiple recipes and requires an explicit choice")]
    AmbiguousRecipes {
        commodity: CommodityId,
        recipes: Vec<RecipeId>,
    },
    #[error("recipe {recipe} has no compatible machine for category {category}")]
    NoCompatibleMachine {
        recipe: RecipeId,
        category: RecipeCategory,
    },
    #[error("recipe {recipe} has multiple compatible machines and requires an explicit choice")]
    AmbiguousMachines {
        recipe: RecipeId,
        machines: Vec<MachineId>,
    },
    #[error("selected production dependencies contain a cycle: {path:?}")]
    Cycle { path: Vec<CommodityId> },
    #[error("calculated {quantity} is invalid: {value}")]
    InvalidCalculatedValue { quantity: &'static str, value: f64 },
}

/// Calculates a single deterministic production chain.
///
/// The planner has no filesystem, terminal, or application-state access. All
/// rates in the returned result remain in units per second.
///
/// # Errors
///
/// Returns [`PlannerError`] when the target is unknown, a recipe or machine
/// choice is ambiguous, no compatible machine exists, dependencies contain a
/// cycle, or arithmetic produces an invalid value.
pub fn calculate(catalog: &Catalog, plan: &FactoryPlan) -> Result<CalculationResult, PlannerError> {
    if catalog.commodity(plan.target().commodity()).is_none() {
        return Err(PlannerError::UnknownTarget {
            commodity: plan.target().commodity().clone(),
        });
    }

    let mut calculation = Calculation::new(catalog, plan);
    calculation.expand(plan.target().commodity(), plan.target().rate_per_second())?;
    calculation.finish()
}

struct Calculation<'a> {
    catalog: &'a Catalog,
    plan: &'a FactoryPlan,
    production_steps: Vec<ProductionStep>,
    external_inputs: BTreeMap<CommodityId, f64>,
    active_path: Vec<CommodityId>,
}

impl<'a> Calculation<'a> {
    fn new(catalog: &'a Catalog, plan: &'a FactoryPlan) -> Self {
        Self {
            catalog,
            plan,
            production_steps: Vec::new(),
            external_inputs: BTreeMap::new(),
            active_path: Vec::new(),
        }
    }

    fn expand(
        &mut self,
        commodity: &CommodityId,
        required_rate: Positive,
    ) -> Result<(), PlannerError> {
        if self.plan.external_inputs().contains(commodity)
            || self.catalog.recipes_for_product(commodity).is_empty()
        {
            return self.add_external_input(commodity, required_rate);
        }

        if let Some(cycle_start) = self
            .active_path
            .iter()
            .position(|active| active == commodity)
        {
            let mut path = self.active_path[cycle_start..].to_vec();
            path.push(commodity.clone());
            return Err(PlannerError::Cycle { path });
        }

        let recipe = self.select_recipe(commodity)?;
        let machine_id = self.select_machine(recipe)?;
        let product_amount = recipe
            .products()
            .iter()
            .filter(|product| product.commodity() == commodity)
            .map(|product| product.amount().get())
            .sum::<f64>();
        let craft_rate = checked_positive(
            required_rate.get() / product_amount,
            "craft rate per second",
        )?;
        let machine = self
            .catalog
            .machine(&machine_id)
            .expect("machine IDs in the category index must resolve");
        let crafts_per_second_per_machine = checked_positive(
            machine.crafting_speed().get() / recipe.duration().get(),
            "crafts per second per machine",
        )?;
        let fractional_machine_count = checked_positive(
            craft_rate.get() / crafts_per_second_per_machine.get(),
            "fractional machine count",
        )?;
        let installed_machine_count =
            checked_installed_machine_count(fractional_machine_count.get())?;

        let ingredients = recipe
            .ingredients()
            .iter()
            .map(|ingredient| {
                checked_positive(
                    craft_rate.get() * ingredient.amount().get(),
                    "ingredient rate per second",
                )
                .map(|rate| CommodityRate {
                    commodity: ingredient.commodity().clone(),
                    rate,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.production_steps.push(ProductionStep {
            planning_product: commodity.clone(),
            recipe: recipe.id().clone(),
            machine: machine_id,
            required_output_rate: required_rate,
            craft_rate,
            fractional_machine_count,
            installed_machine_count,
            ingredients: ingredients.clone(),
        });

        self.active_path.push(commodity.clone());
        for ingredient in ingredients {
            self.expand(ingredient.commodity(), ingredient.rate())?;
        }
        self.active_path.pop();
        Ok(())
    }

    fn select_recipe(&self, commodity: &CommodityId) -> Result<&Recipe, PlannerError> {
        let recipe_ids = self.catalog.recipes_for_product(commodity);
        if recipe_ids.len() > 1 {
            return Err(PlannerError::AmbiguousRecipes {
                commodity: commodity.clone(),
                recipes: recipe_ids.to_vec(),
            });
        }
        Ok(self
            .catalog
            .recipe(&recipe_ids[0])
            .expect("recipe IDs in the product index must resolve"))
    }

    fn select_machine(&self, recipe: &Recipe) -> Result<MachineId, PlannerError> {
        let machine_ids = self.catalog.machines_for_category(recipe.category());
        match machine_ids {
            [] => Err(PlannerError::NoCompatibleMachine {
                recipe: recipe.id().clone(),
                category: recipe.category().clone(),
            }),
            [machine_id] => Ok(machine_id.clone()),
            _ => Err(PlannerError::AmbiguousMachines {
                recipe: recipe.id().clone(),
                machines: machine_ids.to_vec(),
            }),
        }
    }

    fn add_external_input(
        &mut self,
        commodity: &CommodityId,
        rate: Positive,
    ) -> Result<(), PlannerError> {
        let total = self.external_inputs.entry(commodity.clone()).or_default();
        *total += rate.get();
        checked_positive(*total, "external input rate per second")?;
        Ok(())
    }

    fn finish(mut self) -> Result<CalculationResult, PlannerError> {
        self.production_steps
            .sort_by(|left, right| left.planning_product.cmp(&right.planning_product));
        let external_inputs = self
            .external_inputs
            .into_iter()
            .map(|(commodity, rate)| {
                checked_positive(rate, "external input rate per second")
                    .map(|rate| CommodityRate { commodity, rate })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CalculationResult {
            production_steps: self.production_steps,
            external_inputs,
            display_rate_unit: self.plan.display_rate_unit(),
        })
    }
}

fn checked_positive(value: f64, quantity: &'static str) -> Result<Positive, PlannerError> {
    Positive::new(value).map_err(|_| PlannerError::InvalidCalculatedValue { quantity, value })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn checked_installed_machine_count(value: f64) -> Result<u64, PlannerError> {
    let installed = value.ceil();
    if installed >= 2_f64.powi(64) {
        return Err(PlannerError::InvalidCalculatedValue {
            quantity: "installed machine count",
            value: installed,
        });
    }
    // The positive, finite input and exclusive upper-bound check make this
    // rounded value exactly representable as a u64.
    Ok(installed as u64)
}
