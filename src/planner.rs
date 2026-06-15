use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::catalog::{
    Catalog, CommodityId, Finite, Fuel, FuelCategory, FuelId, ItemId, Machine, MachineEnergySource,
    MachineId, ModuleEffect, ModuleId, NonNegative, NumericError, Positive, Recipe, RecipeCategory,
    RecipeId, UnsupportedEnergySource,
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
    recipe_choices: BTreeMap<CommodityId, RecipeId>,
    machine_choices: BTreeMap<RecipeId, MachineId>,
    module_choices: BTreeMap<CommodityId, Vec<ModuleId>>,
    fuel_choices: BTreeMap<CommodityId, FuelId>,
    display_rate_unit: RateUnit,
}

impl FactoryPlan {
    #[must_use]
    pub fn new(target: Target) -> Self {
        Self {
            targets: vec![target],
            external_inputs: BTreeSet::new(),
            recipe_choices: BTreeMap::new(),
            machine_choices: BTreeMap::new(),
            module_choices: BTreeMap::new(),
            fuel_choices: BTreeMap::new(),
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

    pub fn set_recipe_choice(
        &mut self,
        commodity: CommodityId,
        recipe: RecipeId,
    ) -> Option<RecipeId> {
        self.recipe_choices.insert(commodity, recipe)
    }

    pub fn clear_recipe_choice(&mut self, commodity: &CommodityId) -> Option<RecipeId> {
        self.recipe_choices.remove(commodity)
    }

    #[must_use]
    pub fn recipe_choice(&self, commodity: &CommodityId) -> Option<&RecipeId> {
        self.recipe_choices.get(commodity)
    }

    #[must_use]
    pub const fn recipe_choices(&self) -> &BTreeMap<CommodityId, RecipeId> {
        &self.recipe_choices
    }

    pub fn set_machine_choice(
        &mut self,
        recipe: RecipeId,
        machine: MachineId,
    ) -> Option<MachineId> {
        self.machine_choices.insert(recipe, machine)
    }

    pub fn clear_machine_choice(&mut self, recipe: &RecipeId) -> Option<MachineId> {
        self.machine_choices.remove(recipe)
    }

    #[must_use]
    pub fn machine_choice(&self, recipe: &RecipeId) -> Option<&MachineId> {
        self.machine_choices.get(recipe)
    }

    #[must_use]
    pub const fn machine_choices(&self) -> &BTreeMap<RecipeId, MachineId> {
        &self.machine_choices
    }

    pub fn set_modules(
        &mut self,
        commodity: CommodityId,
        modules: impl IntoIterator<Item = ModuleId>,
    ) -> Option<Vec<ModuleId>> {
        let mut modules = modules.into_iter().collect::<Vec<_>>();
        modules.sort();
        if modules.is_empty() {
            self.module_choices.remove(&commodity)
        } else {
            self.module_choices.insert(commodity, modules)
        }
    }

    pub fn clear_modules(&mut self, commodity: &CommodityId) -> Option<Vec<ModuleId>> {
        self.module_choices.remove(commodity)
    }

    #[must_use]
    pub fn modules_for(&self, commodity: &CommodityId) -> &[ModuleId] {
        self.module_choices
            .get(commodity)
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub const fn module_choices(&self) -> &BTreeMap<CommodityId, Vec<ModuleId>> {
        &self.module_choices
    }

    pub fn set_fuel_choice(&mut self, commodity: CommodityId, fuel: FuelId) -> Option<FuelId> {
        self.fuel_choices.insert(commodity, fuel)
    }

    pub fn clear_fuel_choice(&mut self, commodity: &CommodityId) -> Option<FuelId> {
        self.fuel_choices.remove(commodity)
    }

    #[must_use]
    pub fn fuel_choice(&self, commodity: &CommodityId) -> Option<&FuelId> {
        self.fuel_choices.get(commodity)
    }

    #[must_use]
    pub const fn fuel_choices(&self) -> &BTreeMap<CommodityId, FuelId> {
        &self.fuel_choices
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
pub struct ElectricPower {
    fractional_process_watts: Positive,
    installed_full_load_watts: Positive,
}

impl ElectricPower {
    #[must_use]
    pub const fn fractional_process_watts(&self) -> Positive {
        self.fractional_process_watts
    }

    #[must_use]
    pub const fn installed_full_load_watts(&self) -> Positive {
        self.installed_full_load_watts
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuelUsage {
    fuel: FuelId,
    fuel_item: ItemId,
    rate_per_second: Positive,
    burnt_result: Option<CommodityRate>,
}

impl FuelUsage {
    #[must_use]
    pub const fn fuel(&self) -> &FuelId {
        &self.fuel
    }

    #[must_use]
    pub const fn fuel_item(&self) -> &ItemId {
        &self.fuel_item
    }

    #[must_use]
    pub const fn rate_per_second(&self) -> Positive {
        self.rate_per_second
    }

    #[must_use]
    pub const fn burnt_result(&self) -> Option<&CommodityRate> {
        self.burnt_result.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepEnergy {
    Electric(ElectricPower),
    Burner(FuelUsage),
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
    modules: Vec<ModuleId>,
    speed_multiplier: Positive,
    productivity_effect: Finite,
    consumption_multiplier: Positive,
    energy: StepEnergy,
    ingredients: Vec<CommodityRate>,
    products: Vec<CommodityRate>,
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
    pub fn modules(&self) -> &[ModuleId] {
        &self.modules
    }

    #[must_use]
    pub const fn speed_multiplier(&self) -> Positive {
        self.speed_multiplier
    }

    #[must_use]
    pub const fn productivity_effect(&self) -> Finite {
        self.productivity_effect
    }

    #[must_use]
    pub const fn consumption_multiplier(&self) -> Positive {
        self.consumption_multiplier
    }

    #[must_use]
    pub const fn energy(&self) -> &StepEnergy {
        &self.energy
    }

    #[must_use]
    pub fn ingredients(&self) -> &[CommodityRate] {
        &self.ingredients
    }

    #[must_use]
    pub fn products(&self) -> &[CommodityRate] {
        &self.products
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalculationResult {
    production_steps: Vec<ProductionStep>,
    external_inputs: Vec<CommodityRate>,
    surplus: Vec<CommodityRate>,
    electric_power: Option<ElectricPower>,
    burner_fuel_demand: Vec<FuelUsage>,
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
    pub fn surplus(&self) -> &[CommodityRate] {
        &self.surplus
    }

    #[must_use]
    pub const fn electric_power(&self) -> Option<&ElectricPower> {
        self.electric_power.as_ref()
    }

    #[must_use]
    pub fn burner_fuel_demand(&self) -> &[FuelUsage] {
        &self.burner_fuel_demand
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
    #[error("selected recipe {recipe} for {commodity} is not present in the catalog")]
    MissingRecipeChoice {
        commodity: CommodityId,
        recipe: RecipeId,
    },
    #[error("selected recipe {recipe} for {commodity} uses unsupported behavior")]
    UnsupportedRecipeChoice {
        commodity: CommodityId,
        recipe: RecipeId,
    },
    #[error("selected recipe {recipe} does not produce {commodity}")]
    RecipeDoesNotProduceCommodity {
        commodity: CommodityId,
        recipe: RecipeId,
    },
    #[error("recipe {recipe} has no compatible machine for category {category}")]
    NoCompatibleMachine {
        recipe: RecipeId,
        category: RecipeCategory,
    },
    #[error("selected machine {machine} for recipe {recipe} is not present in the catalog")]
    MissingMachineChoice {
        recipe: RecipeId,
        machine: MachineId,
    },
    #[error(
        "selected machine {machine} does not support category {category} required by recipe {recipe}"
    )]
    IncompatibleMachineChoice {
        recipe: RecipeId,
        machine: MachineId,
        category: RecipeCategory,
    },
    #[error(
        "machine {machine} selected for recipe {recipe} uses unsupported energy source {energy_source:?}"
    )]
    UnsupportedMachineEnergySource {
        recipe: RecipeId,
        machine: MachineId,
        energy_source: UnsupportedEnergySource,
    },
    #[error("selected module {module} for {commodity} is not present in the catalog")]
    MissingModuleChoice {
        commodity: CommodityId,
        module: ModuleId,
    },
    #[error("selected module {module} for {commodity} has unsupported effects")]
    UnsupportedModuleChoice {
        commodity: CommodityId,
        module: ModuleId,
    },
    #[error(
        "selected {selected} modules for {commodity}, but machine {machine} has only {slots} slots"
    )]
    TooManyModules {
        commodity: CommodityId,
        machine: MachineId,
        selected: usize,
        slots: u16,
    },
    #[error("machine {machine} does not allow module {module} category {category} for {commodity}")]
    MachineDisallowsModuleCategory {
        commodity: CommodityId,
        machine: MachineId,
        module: ModuleId,
        category: crate::catalog::ModuleCategory,
    },
    #[error("recipe {recipe} does not allow module {module} category {category} for {commodity}")]
    RecipeDisallowsModuleCategory {
        commodity: CommodityId,
        recipe: RecipeId,
        module: ModuleId,
        category: crate::catalog::ModuleCategory,
    },
    #[error("machine {machine} does not allow module {module} effect {effect:?} for {commodity}")]
    MachineDisallowsModuleEffect {
        commodity: CommodityId,
        machine: MachineId,
        module: ModuleId,
        effect: ModuleEffect,
    },
    #[error("recipe {recipe} does not allow module {module} effect {effect:?} for {commodity}")]
    RecipeDisallowsModuleEffect {
        commodity: CommodityId,
        recipe: RecipeId,
        module: ModuleId,
        effect: ModuleEffect,
    },
    #[error("selected fuel {fuel} for {commodity} is not present in the catalog")]
    MissingFuelChoice {
        commodity: CommodityId,
        fuel: FuelId,
    },
    #[error(
        "selected fuel {fuel} category {category} is incompatible with machine {machine} for {commodity}"
    )]
    IncompatibleFuelChoice {
        commodity: CommodityId,
        machine: MachineId,
        fuel: FuelId,
        category: FuelCategory,
    },
    #[error("machine {machine} has no compatible fuel for {commodity}")]
    NoCompatibleFuel {
        commodity: CommodityId,
        machine: MachineId,
    },
    #[error("selected fuel {fuel} for {commodity}, but machine {machine} is not a burner")]
    FuelChoiceForNonBurnerMachine {
        commodity: CommodityId,
        machine: MachineId,
        fuel: FuelId,
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
/// Returns [`PlannerError`] when a target or explicit recipe choice is invalid,
/// no compatible machine exists, a machine choice is ambiguous, dependencies
/// contain a cycle, or arithmetic produces an invalid value.
pub fn calculate(catalog: &Catalog, plan: &FactoryPlan) -> Result<CalculationResult, PlannerError> {
    let target_rates = aggregate_target_rates(plan.targets())?;
    for (commodity, _) in &target_rates {
        if catalog.commodity(commodity).is_none() {
            return Err(PlannerError::UnknownTarget {
                commodity: commodity.clone(),
            });
        }
    }

    let resolved_plan = resolve_plan(catalog, plan, &target_rates)?;
    let mut calculation = Calculation::new(catalog, plan, resolved_plan);
    for (commodity, rate) in target_rates {
        calculation.expand(&commodity, rate)?;
    }
    calculation.finish()
}

struct Calculation<'a> {
    catalog: &'a Catalog,
    plan: &'a FactoryPlan,
    resolved_plan: ResolvedPlan,
    production_steps: BTreeMap<CommodityId, ProductionStepAccumulator>,
    external_inputs: BTreeMap<CommodityId, f64>,
}

impl<'a> Calculation<'a> {
    fn new(catalog: &'a Catalog, plan: &'a FactoryPlan, resolved_plan: ResolvedPlan) -> Self {
        Self {
            catalog,
            plan,
            resolved_plan,
            production_steps: BTreeMap::new(),
            external_inputs: BTreeMap::new(),
        }
    }

    fn expand(
        &mut self,
        commodity: &CommodityId,
        required_rate: Positive,
    ) -> Result<(), PlannerError> {
        if self.plan.external_inputs().contains(commodity) {
            return self.add_external_input(commodity, required_rate);
        }

        let Some(recipe_id) = self.resolved_plan.recipes.get(commodity) else {
            return self.add_external_input(commodity, required_rate);
        };
        let recipe_id = recipe_id.clone();
        let recipe = self
            .catalog
            .recipe(&recipe_id)
            .expect("resolved recipe IDs must remain present in the catalog");
        let machine_id = self
            .resolved_plan
            .machines
            .get(commodity)
            .expect("resolved production recipes must have machines")
            .clone();
        let machine = self
            .catalog
            .machine(&machine_id)
            .expect("selected machine IDs must remain present in the catalog");
        let module_configuration = self.resolve_modules(commodity, recipe, machine)?;
        let product_amount = recipe
            .products()
            .iter()
            .filter(|product| product.commodity() == commodity)
            .map(|product| {
                effective_product_amount(product, module_configuration.productivity_effect)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum::<f64>();
        let craft_rate = checked_positive(
            required_rate.get() / product_amount,
            "craft rate per second",
        )?;
        let crafts_per_second_per_machine = checked_positive(
            machine.crafting_speed().get() * module_configuration.speed_multiplier.get()
                / recipe.duration().get(),
            "crafts per second per machine",
        )?;
        let fractional_machine_count = checked_positive(
            craft_rate.get() / crafts_per_second_per_machine.get(),
            "fractional machine count",
        )?;
        let energy = self.resolve_step_energy(
            commodity,
            machine,
            module_configuration.consumption_multiplier,
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
        let products = aggregate_recipe_products(
            recipe,
            craft_rate,
            module_configuration.productivity_effect,
        )?;

        self.add_production_step(
            commodity,
            &recipe_id,
            &machine_id,
            required_rate,
            craft_rate,
            fractional_machine_count,
            &module_configuration,
            &energy,
            &ingredients,
            &products,
        )?;

        for ingredient in ingredients {
            self.expand(ingredient.commodity(), ingredient.rate())?;
        }
        if let Some(fuel_rate) = energy.fuel_rate(fractional_machine_count)? {
            self.expand(
                &CommodityId::Item(fuel_rate.fuel_item().clone()),
                fuel_rate.rate_per_second(),
            )?;
        }
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
        module_configuration: &ModuleConfiguration,
        energy: &StepEnergyConfiguration,
        ingredients: &[CommodityRate],
        products: &[CommodityRate],
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
                modules: module_configuration.modules.clone(),
                speed_multiplier: module_configuration.speed_multiplier,
                productivity_effect: module_configuration.productivity_effect,
                consumption_multiplier: module_configuration.consumption_multiplier,
                energy: energy.clone(),
                ingredients: BTreeMap::new(),
                products: BTreeMap::new(),
            });
        debug_assert_eq!(&step.recipe, recipe);
        debug_assert_eq!(&step.machine, machine);
        debug_assert_eq!(&step.modules, &module_configuration.modules);
        debug_assert_eq!(
            step.productivity_effect,
            module_configuration.productivity_effect
        );
        debug_assert_eq!(&step.energy, energy);

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
        for product in products {
            let total = step
                .products
                .entry(product.commodity().clone())
                .or_default();
            *total += product.rate().get();
            checked_positive(*total, "aggregated product rate per second")?;
        }
        Ok(())
    }

    fn resolve_step_energy(
        &self,
        commodity: &CommodityId,
        machine: &Machine,
        consumption_multiplier: Positive,
    ) -> Result<StepEnergyConfiguration, PlannerError> {
        let active_watts_per_machine = checked_positive(
            machine.energy_usage().get() * consumption_multiplier.get(),
            "active machine power",
        )?;
        match machine.energy_source() {
            MachineEnergySource::Electric { drain } => Ok(StepEnergyConfiguration::Electric {
                active_watts_per_machine,
                drain_watts_per_machine: *drain,
            }),
            MachineEnergySource::Burner { effectivity, .. } => {
                let fuel_id = self
                    .resolved_plan
                    .fuels
                    .get(commodity)
                    .expect("resolved burner production steps must have fuels");
                let fuel = self
                    .catalog
                    .fuel(fuel_id)
                    .expect("resolved fuel IDs must remain present in the catalog");
                Ok(StepEnergyConfiguration::Burner {
                    fuel: fuel.id().clone(),
                    fuel_item: fuel.item().clone(),
                    active_watts_per_machine,
                    effectivity: *effectivity,
                    fuel_value: fuel.fuel_value(),
                    burnt_result: fuel.burnt_result().cloned(),
                })
            }
            MachineEnergySource::Unsupported(source) => {
                Err(PlannerError::UnsupportedMachineEnergySource {
                    recipe: self
                        .resolved_plan
                        .recipes
                        .get(commodity)
                        .expect("resolved production steps must have recipes")
                        .clone(),
                    machine: machine.id().clone(),
                    energy_source: source.clone(),
                })
            }
        }
    }

    fn resolve_modules(
        &self,
        commodity: &CommodityId,
        recipe: &Recipe,
        machine: &Machine,
    ) -> Result<ModuleConfiguration, PlannerError> {
        let module_ids = self.plan.modules_for(commodity);
        if module_ids.len() > usize::from(machine.module_slots()) {
            return Err(PlannerError::TooManyModules {
                commodity: commodity.clone(),
                machine: machine.id().clone(),
                selected: module_ids.len(),
                slots: machine.module_slots(),
            });
        }

        let mut speed_effect = 0.0;
        let mut productivity_effect = 0.0;
        let mut consumption_effect = 0.0;
        for module_id in module_ids {
            let module = self.catalog.module(module_id).ok_or_else(|| {
                PlannerError::MissingModuleChoice {
                    commodity: commodity.clone(),
                    module: module_id.clone(),
                }
            })?;
            if !module.is_selectable() {
                return Err(PlannerError::UnsupportedModuleChoice {
                    commodity: commodity.clone(),
                    module: module_id.clone(),
                });
            }
            if machine
                .allowed_module_categories()
                .is_some_and(|categories| !categories.contains(module.category()))
            {
                return Err(PlannerError::MachineDisallowsModuleCategory {
                    commodity: commodity.clone(),
                    machine: machine.id().clone(),
                    module: module_id.clone(),
                    category: module.category().clone(),
                });
            }
            if recipe
                .allowed_module_categories()
                .is_some_and(|categories| !categories.contains(module.category()))
            {
                return Err(PlannerError::RecipeDisallowsModuleCategory {
                    commodity: commodity.clone(),
                    recipe: recipe.id().clone(),
                    module: module_id.clone(),
                    category: module.category().clone(),
                });
            }

            for (effect, value) in [
                (ModuleEffect::Speed, module.speed_effect().get()),
                (
                    ModuleEffect::Productivity,
                    module.productivity_effect().get(),
                ),
                (ModuleEffect::Consumption, module.consumption_effect().get()),
            ] {
                if value == 0.0 {
                    continue;
                }
                if !machine.allowed_effects().contains(&effect) {
                    return Err(PlannerError::MachineDisallowsModuleEffect {
                        commodity: commodity.clone(),
                        machine: machine.id().clone(),
                        module: module_id.clone(),
                        effect,
                    });
                }
                if !recipe.allowed_effects().contains(&effect) {
                    return Err(PlannerError::RecipeDisallowsModuleEffect {
                        commodity: commodity.clone(),
                        recipe: recipe.id().clone(),
                        module: module_id.clone(),
                        effect,
                    });
                }
            }

            speed_effect += module.speed_effect().get();
            productivity_effect += module.productivity_effect().get();
            consumption_effect += module.consumption_effect().get();
        }

        let speed_multiplier =
            checked_positive((1.0_f64 + speed_effect).max(0.2), "module speed multiplier")?;
        let productivity_effect = productivity_effect.min(recipe.maximum_productivity().get());
        let productivity_effect =
            Finite::new(productivity_effect).map_err(|_| PlannerError::InvalidCalculatedValue {
                quantity: "module productivity effect",
                value: productivity_effect,
            })?;
        let consumption_multiplier = checked_positive(
            (1.0_f64 + consumption_effect).max(0.2),
            "module consumption multiplier",
        )?;

        Ok(ModuleConfiguration {
            modules: module_ids.to_vec(),
            speed_multiplier,
            productivity_effect,
            consumption_multiplier,
        })
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
        let surplus = aggregate_surplus(&production_steps)?;
        let electric_power = aggregate_electric_power(&production_steps)?;
        let burner_fuel_demand = aggregate_burner_fuel_demand(&production_steps)?;

        Ok(CalculationResult {
            production_steps,
            external_inputs,
            surplus,
            electric_power,
            burner_fuel_demand,
            display_rate_unit: self.plan.display_rate_unit(),
        })
    }
}

fn resolve_plan(
    catalog: &Catalog,
    plan: &FactoryPlan,
    targets: &[(CommodityId, Positive)],
) -> Result<ResolvedPlan, PlannerError> {
    let mut resolver = RecipeResolver::new(catalog, plan);
    for (commodity, _) in targets {
        resolver.visit(commodity)?;
    }
    Ok(ResolvedPlan {
        recipes: resolver.selected_recipes,
        machines: resolver.selected_machines,
        fuels: resolver.selected_fuels,
    })
}

struct ResolvedPlan {
    recipes: BTreeMap<CommodityId, RecipeId>,
    machines: BTreeMap<CommodityId, MachineId>,
    fuels: BTreeMap<CommodityId, FuelId>,
}

struct RecipeResolver<'a> {
    catalog: &'a Catalog,
    plan: &'a FactoryPlan,
    selected_recipes: BTreeMap<CommodityId, RecipeId>,
    selected_machines: BTreeMap<CommodityId, MachineId>,
    selected_fuels: BTreeMap<CommodityId, FuelId>,
    resolved: BTreeSet<CommodityId>,
    active_path: Vec<CommodityId>,
}

impl<'a> RecipeResolver<'a> {
    fn new(catalog: &'a Catalog, plan: &'a FactoryPlan) -> Self {
        Self {
            catalog,
            plan,
            selected_recipes: BTreeMap::new(),
            selected_machines: BTreeMap::new(),
            selected_fuels: BTreeMap::new(),
            resolved: BTreeSet::new(),
            active_path: Vec::new(),
        }
    }

    fn visit(&mut self, commodity: &CommodityId) -> Result<(), PlannerError> {
        if self.plan.external_inputs().contains(commodity) || self.resolved.contains(commodity) {
            return Ok(());
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

        let Some(recipe) = self.select_recipe(commodity)? else {
            self.resolved.insert(commodity.clone());
            return Ok(());
        };
        let recipe_id = recipe.id().clone();
        let machine_id = select_machine(self.catalog, self.plan, recipe)?;
        let machine = self
            .catalog
            .machine(&machine_id)
            .expect("selected machine IDs must remain present in the catalog");
        let fuel_id = select_fuel(self.catalog, self.plan, commodity, machine)?;
        let mut dependencies = recipe
            .ingredients()
            .iter()
            .map(|ingredient| ingredient.commodity().clone())
            .collect::<Vec<_>>();
        if let Some(fuel_id) = &fuel_id {
            let fuel = self
                .catalog
                .fuel(fuel_id)
                .expect("selected fuel IDs must remain present in the catalog");
            dependencies.push(CommodityId::Item(fuel.item().clone()));
        }
        self.selected_recipes.insert(commodity.clone(), recipe_id);
        self.selected_machines.insert(commodity.clone(), machine_id);
        if let Some(fuel_id) = fuel_id {
            self.selected_fuels.insert(commodity.clone(), fuel_id);
        }

        self.active_path.push(commodity.clone());
        for dependency in dependencies {
            self.visit(&dependency)?;
        }
        self.active_path.pop();
        self.resolved.insert(commodity.clone());
        Ok(())
    }

    fn select_recipe(&self, commodity: &CommodityId) -> Result<Option<&Recipe>, PlannerError> {
        if let Some(recipe_id) = self.plan.recipe_choice(commodity) {
            let recipe = self.catalog.recipe(recipe_id).ok_or_else(|| {
                PlannerError::MissingRecipeChoice {
                    commodity: commodity.clone(),
                    recipe: recipe_id.clone(),
                }
            })?;
            if !recipe.supported() {
                return Err(PlannerError::UnsupportedRecipeChoice {
                    commodity: commodity.clone(),
                    recipe: recipe_id.clone(),
                });
            }
            if !recipe
                .products()
                .iter()
                .any(|product| product.commodity() == commodity)
            {
                return Err(PlannerError::RecipeDoesNotProduceCommodity {
                    commodity: commodity.clone(),
                    recipe: recipe_id.clone(),
                });
            }
            return Ok(Some(recipe));
        }

        Ok(self
            .catalog
            .recipes_for_product(commodity)
            .iter()
            .filter_map(|recipe_id| self.catalog.recipe(recipe_id))
            .filter(|recipe| recipe.supported())
            .min_by(|left, right| {
                default_recipe_rank(left, commodity)
                    .cmp(&default_recipe_rank(right, commodity))
                    .then_with(|| left.id().cmp(right.id()))
            }))
    }
}

fn select_machine(
    catalog: &Catalog,
    plan: &FactoryPlan,
    recipe: &Recipe,
) -> Result<MachineId, PlannerError> {
    if let Some(machine_id) = plan.machine_choice(recipe.id()) {
        let machine =
            catalog
                .machine(machine_id)
                .ok_or_else(|| PlannerError::MissingMachineChoice {
                    recipe: recipe.id().clone(),
                    machine: machine_id.clone(),
                })?;
        if !machine.supports_category(recipe.category()) {
            return Err(PlannerError::IncompatibleMachineChoice {
                recipe: recipe.id().clone(),
                machine: machine_id.clone(),
                category: recipe.category().clone(),
            });
        }
        ensure_supported_energy_source(recipe, machine)?;
        return Ok(machine_id.clone());
    }

    let compatible = catalog
        .machines_for_category(recipe.category())
        .iter()
        .filter_map(|machine_id| catalog.machine(machine_id))
        .collect::<Vec<_>>();
    let fastest_supported = compatible
        .iter()
        .copied()
        .filter(|machine| !matches!(machine.energy_source(), MachineEnergySource::Unsupported(_)))
        .max_by(|left, right| compare_machines(left, right));
    if let Some(machine) = fastest_supported {
        return Ok(machine.id().clone());
    }

    let Some(machine) = compatible
        .into_iter()
        .max_by(|left, right| compare_machines(left, right))
    else {
        return Err(PlannerError::NoCompatibleMachine {
            recipe: recipe.id().clone(),
            category: recipe.category().clone(),
        });
    };
    ensure_supported_energy_source(recipe, machine)?;
    unreachable!("unsupported machines must return an error")
}

fn compare_machines(left: &Machine, right: &Machine) -> std::cmp::Ordering {
    left.crafting_speed()
        .get()
        .total_cmp(&right.crafting_speed().get())
        .then_with(|| right.id().cmp(left.id()))
}

fn ensure_supported_energy_source(recipe: &Recipe, machine: &Machine) -> Result<(), PlannerError> {
    if let MachineEnergySource::Unsupported(source) = machine.energy_source() {
        return Err(PlannerError::UnsupportedMachineEnergySource {
            recipe: recipe.id().clone(),
            machine: machine.id().clone(),
            energy_source: source.clone(),
        });
    }
    Ok(())
}

fn select_fuel(
    catalog: &Catalog,
    plan: &FactoryPlan,
    commodity: &CommodityId,
    machine: &Machine,
) -> Result<Option<FuelId>, PlannerError> {
    match machine.energy_source() {
        MachineEnergySource::Electric { .. } => {
            if let Some(fuel) = plan.fuel_choice(commodity) {
                return Err(PlannerError::FuelChoiceForNonBurnerMachine {
                    commodity: commodity.clone(),
                    machine: machine.id().clone(),
                    fuel: fuel.clone(),
                });
            }
            Ok(None)
        }
        MachineEnergySource::Burner {
            fuel_categories, ..
        } => {
            if let Some(fuel_id) = plan.fuel_choice(commodity) {
                let fuel =
                    catalog
                        .fuel(fuel_id)
                        .ok_or_else(|| PlannerError::MissingFuelChoice {
                            commodity: commodity.clone(),
                            fuel: fuel_id.clone(),
                        })?;
                if !fuel_categories.contains(fuel.category()) {
                    return Err(PlannerError::IncompatibleFuelChoice {
                        commodity: commodity.clone(),
                        machine: machine.id().clone(),
                        fuel: fuel.id().clone(),
                        category: fuel.category().clone(),
                    });
                }
                return Ok(Some(fuel.id().clone()));
            }

            catalog
                .fuels()
                .filter(|fuel| fuel_categories.contains(fuel.category()))
                .max_by(|left, right| compare_fuels(left, right))
                .map(|fuel| Some(fuel.id().clone()))
                .ok_or_else(|| PlannerError::NoCompatibleFuel {
                    commodity: commodity.clone(),
                    machine: machine.id().clone(),
                })
        }
        MachineEnergySource::Unsupported(_) => {
            unreachable!("unsupported machine energy sources are rejected during selection")
        }
    }
}

fn compare_fuels(left: &Fuel, right: &Fuel) -> std::cmp::Ordering {
    left.fuel_value()
        .get()
        .total_cmp(&right.fuel_value().get())
        .then_with(|| right.id().cmp(left.id()))
}

fn default_recipe_rank(recipe: &Recipe, commodity: &CommodityId) -> (u8, u8) {
    let visibility_rank = u8::from(!recipe.visible());
    let product_rank = if recipe.main_product() == Some(commodity) {
        0
    } else if recipe
        .products()
        .iter()
        .all(|product| product.commodity() == commodity)
    {
        1
    } else {
        2
    };
    (visibility_rank, product_rank)
}

struct ProductionStepAccumulator {
    planning_product: CommodityId,
    recipe: RecipeId,
    machine: MachineId,
    required_output_rate: f64,
    craft_rate: f64,
    fractional_machine_count: f64,
    modules: Vec<ModuleId>,
    speed_multiplier: Positive,
    productivity_effect: Finite,
    consumption_multiplier: Positive,
    energy: StepEnergyConfiguration,
    ingredients: BTreeMap<CommodityId, f64>,
    products: BTreeMap<CommodityId, f64>,
}

impl ProductionStepAccumulator {
    fn finish(self) -> Result<ProductionStep, PlannerError> {
        let fractional_machine_count = checked_positive(
            self.fractional_machine_count,
            "aggregated fractional machine count",
        )?;
        let installed_machine_count =
            checked_installed_machine_count(fractional_machine_count.get())?;
        let energy = self
            .energy
            .finish(fractional_machine_count, installed_machine_count)?;
        let ingredients = self
            .ingredients
            .into_iter()
            .map(|(commodity, rate)| {
                checked_positive(rate, "aggregated ingredient rate per second")
                    .map(|rate| CommodityRate { commodity, rate })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let products = self
            .products
            .into_iter()
            .map(|(commodity, rate)| {
                checked_positive(rate, "aggregated product rate per second")
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
            modules: self.modules,
            speed_multiplier: self.speed_multiplier,
            productivity_effect: self.productivity_effect,
            consumption_multiplier: self.consumption_multiplier,
            energy,
            ingredients,
            products,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum StepEnergyConfiguration {
    Electric {
        active_watts_per_machine: Positive,
        drain_watts_per_machine: NonNegative,
    },
    Burner {
        fuel: FuelId,
        fuel_item: ItemId,
        active_watts_per_machine: Positive,
        effectivity: Positive,
        fuel_value: Positive,
        burnt_result: Option<ItemId>,
    },
}

impl StepEnergyConfiguration {
    fn fuel_rate(
        &self,
        fractional_machine_count: Positive,
    ) -> Result<Option<FuelUsage>, PlannerError> {
        let Self::Burner {
            fuel,
            fuel_item,
            active_watts_per_machine,
            effectivity,
            fuel_value,
            burnt_result,
        } = self
        else {
            return Ok(None);
        };
        let rate_per_second = checked_positive(
            active_watts_per_machine.get() * fractional_machine_count.get()
                / (effectivity.get() * fuel_value.get()),
            "burner fuel rate per second",
        )?;
        let burnt_result = burnt_result.as_ref().map(|item| CommodityRate {
            commodity: CommodityId::Item(item.clone()),
            rate: rate_per_second,
        });
        Ok(Some(FuelUsage {
            fuel: fuel.clone(),
            fuel_item: fuel_item.clone(),
            rate_per_second,
            burnt_result,
        }))
    }

    fn finish(
        self,
        fractional_machine_count: Positive,
        installed_machine_count: u64,
    ) -> Result<StepEnergy, PlannerError> {
        match self {
            Self::Electric {
                active_watts_per_machine,
                drain_watts_per_machine,
            } => {
                #[allow(clippy::cast_precision_loss)]
                let installed_machine_count = installed_machine_count as f64;
                let fractional_process_watts = checked_positive(
                    active_watts_per_machine.get() * fractional_machine_count.get()
                        + drain_watts_per_machine.get() * installed_machine_count,
                    "fractional-process electric power",
                )?;
                let installed_full_load_watts = checked_positive(
                    (active_watts_per_machine.get() + drain_watts_per_machine.get())
                        * installed_machine_count,
                    "installed full-load electric power",
                )?;
                Ok(StepEnergy::Electric(ElectricPower {
                    fractional_process_watts,
                    installed_full_load_watts,
                }))
            }
            burner @ Self::Burner { .. } => Ok(StepEnergy::Burner(
                burner
                    .fuel_rate(fractional_machine_count)?
                    .expect("burner energy configuration must produce fuel usage"),
            )),
        }
    }
}

fn aggregate_recipe_products(
    recipe: &Recipe,
    craft_rate: Positive,
    productivity_effect: Finite,
) -> Result<Vec<CommodityRate>, PlannerError> {
    let mut rates = BTreeMap::<CommodityId, f64>::new();
    for product in recipe.products() {
        let rate = craft_rate.get() * effective_product_amount(product, productivity_effect)?;
        checked_positive(rate, "product rate per second")?;
        let total = rates.entry(product.commodity().clone()).or_default();
        *total += rate;
        checked_positive(*total, "aggregated product rate per second")?;
    }
    rates
        .into_iter()
        .map(|(commodity, rate)| {
            checked_positive(rate, "aggregated product rate per second")
                .map(|rate| CommodityRate { commodity, rate })
        })
        .collect()
}

struct ModuleConfiguration {
    modules: Vec<ModuleId>,
    speed_multiplier: Positive,
    productivity_effect: Finite,
    consumption_multiplier: Positive,
}

fn effective_product_amount(
    product: &crate::catalog::Product,
    productivity_effect: Finite,
) -> Result<f64, PlannerError> {
    let amount =
        product.amount().get() + productivity_effect.get() * product.productivity_amount().get();
    checked_positive(amount, "effective product amount per craft").map(Positive::get)
}

fn aggregate_surplus(
    production_steps: &[ProductionStep],
) -> Result<Vec<CommodityRate>, PlannerError> {
    let mut surplus = BTreeMap::<CommodityId, f64>::new();
    for step in production_steps {
        for product in step.products() {
            if product.commodity() == step.planning_product() {
                continue;
            }
            let total = surplus.entry(product.commodity().clone()).or_default();
            *total += product.rate().get();
            checked_positive(*total, "aggregated surplus rate per second")?;
        }
        if let StepEnergy::Burner(fuel_usage) = step.energy()
            && let Some(burnt_result) = fuel_usage.burnt_result()
        {
            let total = surplus.entry(burnt_result.commodity().clone()).or_default();
            *total += burnt_result.rate().get();
            checked_positive(*total, "aggregated burnt-result surplus rate per second")?;
        }
    }
    surplus
        .into_iter()
        .map(|(commodity, rate)| {
            checked_positive(rate, "aggregated surplus rate per second")
                .map(|rate| CommodityRate { commodity, rate })
        })
        .collect()
}

fn aggregate_electric_power(
    production_steps: &[ProductionStep],
) -> Result<Option<ElectricPower>, PlannerError> {
    let mut fractional_process_watts = 0.0;
    let mut installed_full_load_watts = 0.0;
    let mut has_electric_power = false;
    for step in production_steps {
        if let StepEnergy::Electric(power) = step.energy() {
            has_electric_power = true;
            fractional_process_watts += power.fractional_process_watts().get();
            checked_positive(
                fractional_process_watts,
                "aggregated fractional-process electric power",
            )?;
            installed_full_load_watts += power.installed_full_load_watts().get();
            checked_positive(
                installed_full_load_watts,
                "aggregated installed full-load electric power",
            )?;
        }
    }

    if has_electric_power {
        Ok(Some(ElectricPower {
            fractional_process_watts: checked_positive(
                fractional_process_watts,
                "aggregated fractional-process electric power",
            )?,
            installed_full_load_watts: checked_positive(
                installed_full_load_watts,
                "aggregated installed full-load electric power",
            )?,
        }))
    } else {
        Ok(None)
    }
}

fn aggregate_burner_fuel_demand(
    production_steps: &[ProductionStep],
) -> Result<Vec<FuelUsage>, PlannerError> {
    let mut rates = BTreeMap::<FuelId, FuelUsageAccumulator>::new();
    for step in production_steps {
        if let StepEnergy::Burner(fuel_usage) = step.energy() {
            let entry =
                rates
                    .entry(fuel_usage.fuel().clone())
                    .or_insert_with(|| FuelUsageAccumulator {
                        fuel: fuel_usage.fuel().clone(),
                        fuel_item: fuel_usage.fuel_item().clone(),
                        rate_per_second: 0.0,
                        burnt_result: fuel_usage
                            .burnt_result()
                            .map(|rate| rate.commodity().clone()),
                    });
            debug_assert_eq!(entry.fuel_item, *fuel_usage.fuel_item());
            debug_assert_eq!(
                entry.burnt_result.as_ref(),
                fuel_usage
                    .burnt_result()
                    .map(crate::planner::CommodityRate::commodity)
            );
            entry.rate_per_second += fuel_usage.rate_per_second().get();
            checked_positive(
                entry.rate_per_second,
                "aggregated burner fuel rate per second",
            )?;
        }
    }

    rates
        .into_values()
        .map(FuelUsageAccumulator::finish)
        .collect()
}

struct FuelUsageAccumulator {
    fuel: FuelId,
    fuel_item: ItemId,
    rate_per_second: f64,
    burnt_result: Option<CommodityId>,
}

impl FuelUsageAccumulator {
    fn finish(self) -> Result<FuelUsage, PlannerError> {
        let rate_per_second = checked_positive(
            self.rate_per_second,
            "aggregated burner fuel rate per second",
        )?;
        Ok(FuelUsage {
            fuel: self.fuel,
            fuel_item: self.fuel_item,
            rate_per_second,
            burnt_result: self.burnt_result.map(|commodity| CommodityRate {
                commodity,
                rate: rate_per_second,
            }),
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
