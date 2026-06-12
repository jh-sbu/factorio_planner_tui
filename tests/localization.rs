use std::io::Cursor;

use factorio_planner_tui::catalog::{
    BeltId, CommodityId, FluidId, FuelId, ItemId, MachineId, ModuleId, RecipeId,
};
use factorio_planner_tui::import::{
    LocaleError, LocalePrototypeKind, PrototypeLocale, parse_data_raw, parse_data_raw_with_locale,
    parse_prototype_locale,
};

fn item(name: &str) -> CommodityId {
    CommodityId::Item(ItemId::new(name).unwrap())
}

fn fluid(name: &str) -> CommodityId {
    CommodityId::Fluid(FluidId::new(name).unwrap())
}

fn locale() -> PrototypeLocale {
    parse_prototype_locale([
        (
            LocalePrototypeKind::Item,
            Cursor::new(include_str!("fixtures/prototype-locale/item-locale.json")),
        ),
        (
            LocalePrototypeKind::Fluid,
            Cursor::new(include_str!("fixtures/prototype-locale/fluid-locale.json")),
        ),
        (
            LocalePrototypeKind::Recipe,
            Cursor::new(include_str!("fixtures/prototype-locale/recipe-locale.json")),
        ),
        (
            LocalePrototypeKind::Entity,
            Cursor::new(include_str!("fixtures/prototype-locale/entity-locale.json")),
        ),
    ])
    .unwrap()
}

fn localized_catalog() -> factorio_planner_tui::catalog::Catalog {
    parse_data_raw_with_locale(
        Cursor::new(include_str!("fixtures/localization-data-raw.json")),
        &locale(),
    )
    .unwrap()
    .into_catalog()
}

#[test]
fn attaches_localized_names_to_every_supported_record_type() {
    let catalog = localized_catalog();

    let iron_plate = catalog.commodity(&item("iron-plate")).unwrap();
    assert_eq!(iron_plate.localized_name(), Some("Iron plate"));
    assert_eq!(iron_plate.display_name(), "Iron plate");
    assert_eq!(
        catalog.commodity(&fluid("water")).unwrap().display_name(),
        "Fresh water"
    );
    assert_eq!(
        catalog
            .recipe(&RecipeId::new("iron-plate").unwrap())
            .unwrap()
            .display_name(),
        "Smelt iron plate"
    );
    assert_eq!(
        catalog
            .machine(&MachineId::new("assembling-machine-1").unwrap())
            .unwrap()
            .display_name(),
        "Assembly machine"
    );
    assert_eq!(
        catalog
            .module(&ModuleId::new("speed-module").unwrap())
            .unwrap()
            .display_name(),
        "Speed module"
    );
    assert_eq!(
        catalog
            .fuel(&FuelId::new("coal").unwrap())
            .unwrap()
            .display_name(),
        "Coal fuel"
    );
    assert_eq!(
        catalog
            .belt(&BeltId::new("transport-belt").unwrap())
            .unwrap()
            .display_name(),
        "Basic transport belt"
    );
}

#[test]
fn falls_back_to_internal_ids_without_a_localized_name() {
    let without_locale = parse_data_raw(Cursor::new(include_str!(
        "fixtures/localization-data-raw.json"
    )))
    .unwrap();

    assert_eq!(
        without_locale
            .catalog()
            .commodity(&item("iron-plate"))
            .unwrap()
            .localized_name(),
        None
    );
    assert_eq!(
        without_locale
            .catalog()
            .commodity(&item("iron-plate"))
            .unwrap()
            .display_name(),
        "iron-plate"
    );
    assert_eq!(
        localized_catalog()
            .commodity(&item("copper-plate"))
            .unwrap()
            .display_name(),
        "copper-plate"
    );
}

#[test]
fn keeps_item_and_fluid_locale_names_separate_for_identical_ids() {
    let catalog = localized_catalog();

    assert_eq!(
        catalog
            .commodity(&item("shared-name"))
            .unwrap()
            .display_name(),
        "Shared item"
    );
    assert_eq!(
        catalog
            .commodity(&fluid("shared-name"))
            .unwrap()
            .display_name(),
        "Shared fluid"
    );
}

#[test]
fn searches_localized_names_and_internal_ids_deterministically() {
    let catalog = localized_catalog();

    assert_eq!(
        catalog
            .search_commodities("IRON")
            .into_iter()
            .map(|commodity| commodity.id().clone())
            .collect::<Vec<_>>(),
        vec![item("iron-ore"), item("iron-plate")]
    );
    assert_eq!(catalog.search_commodities("fresh").len(), 1);
    assert_eq!(catalog.search_commodities("copper-plate").len(), 1);
    assert_eq!(
        catalog
            .search_recipes("iron-plate")
            .into_iter()
            .map(|recipe| recipe.id().clone())
            .collect::<Vec<_>>(),
        vec![RecipeId::new("iron-plate").unwrap()]
    );
    assert_eq!(
        catalog
            .search_machines("assembly")
            .into_iter()
            .map(|machine| machine.id().clone())
            .collect::<Vec<_>>(),
        vec![MachineId::new("assembling-machine-1").unwrap()]
    );
    assert_eq!(catalog.search_modules("speed").len(), 1);
    assert_eq!(catalog.search_fuels("COAL").len(), 1);
    assert_eq!(catalog.search_belts("basic").len(), 1);
    assert_eq!(
        catalog.search_commodities("").len(),
        catalog.commodities().len()
    );
}

#[test]
fn localization_does_not_change_authoritative_ids_or_indexes() {
    let catalog = localized_catalog();
    let iron_plate = item("iron-plate");
    let recipe_id = RecipeId::new("iron-plate").unwrap();

    assert_eq!(catalog.commodity(&iron_plate).unwrap().id(), &iron_plate);
    assert_eq!(
        catalog.recipe(&recipe_id).unwrap().id().as_str(),
        "iron-plate"
    );
    assert_eq!(catalog.recipes_for_product(&iron_plate), &[recipe_id]);
}

#[test]
fn ignores_unknown_locale_entries_and_descriptions() {
    let catalog = localized_catalog();

    assert!(catalog.commodity(&item("unknown-item")).is_none());
    assert_eq!(
        catalog
            .commodity(&item("iron-plate"))
            .unwrap()
            .display_name(),
        "Iron plate"
    );
}

#[test]
fn reports_malformed_locale_json_with_file_context() {
    let error =
        parse_prototype_locale([(LocalePrototypeKind::Item, Cursor::new("{\n  \"names\": {"))])
            .unwrap_err();

    assert!(matches!(
        error,
        LocaleError::Json {
            prototype_kind: LocalePrototypeKind::Item,
            line,
            column,
            ..
        } if line >= 2 && column >= 1
    ));
}

#[test]
fn rejects_malformed_locale_names_maps_with_file_context() {
    let error =
        parse_prototype_locale([(LocalePrototypeKind::Entity, Cursor::new(r#"{"names": []}"#))])
            .unwrap_err();

    assert_eq!(
        error,
        LocaleError::InvalidData {
            prototype_kind: LocalePrototypeKind::Entity,
            path: "/names".into(),
            message: "names must be a JSON object".into(),
        }
    );
}
