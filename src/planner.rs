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
    targets: Vec<Target>,
    external_inputs: BTreeSet<CommodityId>,
    display_rate_unit: RateUnit,
}

impl FactoryPlan {
    #[must_use]
    pub fn new(target: Target) -> Self {
        Self {
            targets: vec![target],
            external_inputs: BTreeSet::new(),
            display_rate_unit: RateUnit::default(),
        }
    }

    pub fn add_target(&mut self, target: Target) {
        self.targets.push(target);
    }

    /// Replaces a target by its stable position in the plan.
    ///
    /// # Errors
    ///
    /// Returns [`PlanEditError::TargetIndexOutOfBounds`] when `index` does not
    /// identify an existing target.
    pub fn replace_target(
        &mut self,
        index: usize,
        target: Target,
    ) -> Result<Target, PlanEditError> {
        let len = self.targets.len();
        let existing = self
            .targets
            .get_mut(index)
            .ok_or(PlanEditError::TargetIndexOutOfBounds { index, len })?;
        Ok(std::mem::replace(existing, target))
    }

    /// Removes a target by its stable position in the plan.
    ///
    /// # Errors
    ///
    /// Returns [`PlanEditError::TargetIndexOutOfBounds`] when `index` does not
    /// identify an existing target, or [`PlanEditError::CannotRemoveLastTarget`]
    /// when removal would leave the plan without a target.
    pub fn remove_target(&mut self, index: usize) -> Result<Target, PlanEditError> {
        if index >= self.targets.len() {
            return Err(PlanEditError::TargetIndexOutOfBounds {
                index,
                len: self.targets.len(),
            });
        }
        if self.targets.len() == 1 {
            return Err(PlanEditError::CannotRemoveLastTarget);
        }
        Ok(self.targets.remove(index))
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
    pub fn targets(&self) -> &[Target] {
        &self.targets
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

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PlanEditError {
    #[error("target index {index} is out of bounds for {len} targets")]
    TargetIndexOutOfBounds { index: usize, len: usize },
    #[error("a factory plan must contain at least one target")]
    CannotRemoveLastTarget,
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

/// Calculates deterministic production chains for all targets in a plan.
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
    let target_rates = aggregate_target_rates(plan.targets())?;
    for (commodity, _) in &target_rates {
        if catalog.commodity(commodity).is_none() {
            return Err(PlannerError::UnknownTarget {
                commodity: commodity.clone(),
            });
        }
    }

    let mut calculation = Calculation::new(catalog, plan);
    for (commodity, rate) in target_rates {
        calculation.expand(&commodity, rate)?;
    }
    calculation.finish()
}

struct Calculation<'a> {
    catalog: &'a Catalog,
    plan: &'a FactoryPlan,
    production_steps: BTreeMap<CommodityId, ProductionStepAccumulator>,
    external_inputs: BTreeMap<CommodityId, f64>,
    active_path: Vec<CommodityId>,
}

impl<'a> Calculation<'a> {
    fn new(catalog: &'a Catalog, plan: &'a FactoryPlan) -> Self {
        Self {
            catalog,
            plan,
            production_steps: BTreeMap::new(),
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
        let recipe_id = recipe.id().clone();
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

        self.add_production_step(
            commodity,
            &recipe_id,
            &machine_id,
            required_rate,
            craft_rate,
            fractional_machine_count,
            &ingredients,
        )?;

        self.active_path.push(commodity.clone());
        for ingredient in ingredients {
            self.expand(ingredient.commodity(), ingredient.rate())?;
        }
        self.active_path.pop();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_production_step(
        &mut self,
        commodity: &CommodityId,
        recipe: &RecipeId,
        machine: &MachineId,
        required_output_rate: Positive,
        craft_rate: Positive,
        fractional_machine_count: Positive,
        ingredients: &[CommodityRate],
    ) -> Result<(), PlannerError> {
        let step = self
            .production_steps
            .entry(commodity.clone())
            .or_insert_with(|| ProductionStepAccumulator {
                planning_product: commodity.clone(),
                recipe: recipe.clone(),
                machine: machine.clone(),
                required_output_rate: 0.0,
                craft_rate: 0.0,
                fractional_machine_count: 0.0,
                ingredients: BTreeMap::new(),
            });
        debug_assert_eq!(&step.recipe, recipe);
        debug_assert_eq!(&step.machine, machine);

        step.required_output_rate += required_output_rate.get();
        checked_positive(
            step.required_output_rate,
            "aggregated required output rate per second",
        )?;
        step.craft_rate += craft_rate.get();
        checked_positive(step.craft_rate, "aggregated craft rate per second")?;
        step.fractional_machine_count += fractional_machine_count.get();
        checked_positive(
            step.fractional_machine_count,
            "aggregated fractional machine count",
        )?;

        for ingredient in ingredients {
            let total = step
                .ingredients
                .entry(ingredient.commodity().clone())
                .or_default();
            *total += ingredient.rate().get();
            checked_positive(*total, "aggregated ingredient rate per second")?;
        }
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

    fn finish(self) -> Result<CalculationResult, PlannerError> {
        let production_steps = self
            .production_steps
            .into_values()
            .map(ProductionStepAccumulator::finish)
            .collect::<Result<Vec<_>, _>>()?;
        let external_inputs = self
            .external_inputs
            .into_iter()
            .map(|(commodity, rate)| {
                checked_positive(rate, "external input rate per second")
                    .map(|rate| CommodityRate { commodity, rate })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CalculationResult {
            production_steps,
            external_inputs,
            display_rate_unit: self.plan.display_rate_unit(),
        })
    }
}

struct ProductionStepAccumulator {
    planning_product: CommodityId,
    recipe: RecipeId,
    machine: MachineId,
    required_output_rate: f64,
    craft_rate: f64,
    fractional_machine_count: f64,
    ingredients: BTreeMap<CommodityId, f64>,
}

impl ProductionStepAccumulator {
    fn finish(self) -> Result<ProductionStep, PlannerError> {
        let fractional_machine_count = checked_positive(
            self.fractional_machine_count,
            "aggregated fractional machine count",
        )?;
        let installed_machine_count =
            checked_installed_machine_count(fractional_machine_count.get())?;
        let ingredients = self
            .ingredients
            .into_iter()
            .map(|(commodity, rate)| {
                checked_positive(rate, "aggregated ingredient rate per second")
                    .map(|rate| CommodityRate { commodity, rate })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ProductionStep {
            planning_product: self.planning_product,
            recipe: self.recipe,
            machine: self.machine,
            required_output_rate: checked_positive(
                self.required_output_rate,
                "aggregated required output rate per second",
            )?,
            craft_rate: checked_positive(self.craft_rate, "aggregated craft rate per second")?,
            fractional_machine_count,
            installed_machine_count,
            ingredients,
        })
    }
}

fn aggregate_target_rates(
    targets: &[Target],
) -> Result<Vec<(CommodityId, Positive)>, PlannerError> {
    let mut rates_by_commodity = BTreeMap::<CommodityId, Vec<f64>>::new();
    for target in targets {
        rates_by_commodity
            .entry(target.commodity().clone())
            .or_default()
            .push(target.rate_per_second().get());
    }

    rates_by_commodity
        .into_iter()
        .map(|(commodity, mut rates)| {
            rates.sort_by(f64::total_cmp);
            checked_positive(rates.into_iter().sum(), "aggregated target rate per second")
                .map(|rate| (commodity, rate))
        })
        .collect()
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
