use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Catalog {
    operators: Vec<Unit>,
}
#[derive(Deserialize)]
struct Unit {
    id: String,
    name: String,
}

pub fn resolve_name(value: &str) -> Option<&'static str> {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../data/operators.json")).expect("内置单位目录无效")
    });
    let mut matches = catalog
        .operators
        .iter()
        .filter(|unit| unit.name == value.trim());
    let unit = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(unit.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chinese_units_resolve_without_guessing_unknown_names() {
        assert_eq!(resolve_name("望"), Some("char_2027_wang"));
        assert_eq!(resolve_name("赤刃明霄陈"), Some("char_1050_chen3"));
        assert_eq!(resolve_name("棋子"), Some("token_10064_wang_stone1"));
        assert_eq!(resolve_name("不存在的单位"), None);
    }
}
