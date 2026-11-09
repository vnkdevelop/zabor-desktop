use std::collections::BTreeMap;

pub struct Catalog(BTreeMap<String, String>);

impl Catalog {
    pub fn load(language: &str) -> Result<Self, String> {
        let source = match language {
            "ru" => include_str!("../assets/locales/ru.json"),
            "en" => include_str!("../assets/locales/en.json"),
            _ => return Err(format!("Unsupported locale: {language}")),
        };
        serde_json::from_str(source)
            .map(Self)
            .map_err(|error| error.to_string())
    }

    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn megabytes(&self, bytes: u64) -> String {
        if bytes == 0 {
            return format!("0 {}", self.get("mb"));
        }
        format!("{:.1} {}", bytes as f64 / 1_048_576.0, self.get("mb"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference;

    #[test]
    fn catalogs_match_react_keys_and_values() {
        let snapshot = reference::Snapshot::admit("catalogs");
        for language in ["ru", "en"] {
            let catalog = Catalog::load(language).unwrap();
            let reference = snapshot.catalog(language)["updater"].as_object().unwrap();
            assert_eq!(catalog.0.len(), reference.len());
            for (key, value) in reference {
                assert_eq!(catalog.get(key), value.as_str().unwrap());
            }
        }
    }

    #[test]
    fn byte_labels_keep_legacy_format() {
        let catalog = Catalog::load("ru").unwrap();
        assert_eq!(catalog.megabytes(0), "0 мб");
        assert_eq!(catalog.megabytes(10_485_760), "10.0 мб");
    }
}
