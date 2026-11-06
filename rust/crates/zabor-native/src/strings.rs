use serde_json::Value;

pub struct Strings {
    language: String,
    catalog: Value,
}

impl Strings {
    pub fn new(language: &str) -> Result<Self, String> {
        let source = match language {
            "ru" => include_str!("../../../../src/renderer/src/locales/ru.json"),
            "en" => include_str!("../../../../src/renderer/src/locales/en.json"),
            _ => return Err("Unsupported language".into()),
        };
        Ok(Self {
            language: language.to_owned(),
            catalog: serde_json::from_str(source).map_err(|error| error.to_string())?,
        })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn text(&self, key: &str) -> String {
        let mut value = &self.catalog;
        for part in key.split('.') {
            value = &value[part];
        }
        value.as_str().unwrap_or(key).to_owned()
    }

    pub fn format(&self, key: &str, parameters: &[(&str, &str)]) -> String {
        let mut text = self.text(key);
        for (name, value) in parameters {
            text = text.replace(&format!("{{{{{name}}}}}"), value);
        }
        text
    }

    pub fn auth_error(&self, error: &crate::auth::AuthError) -> String {
        match error {
            crate::auth::AuthError::Throttled(minutes) => {
                self.format(error.key(), &[("minutes", &minutes.to_string())])
            }
            _ => self.text(error.key()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_views_reference_existing_bilingual_keys() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_none_or(|extension| extension != "slint")
            {
                continue;
            }
            let source = std::fs::read_to_string(path).unwrap();
            for key in source.split('"').skip(1).step_by(2).filter(|key| {
                [
                    "auth.",
                    "settings.",
                    "main.",
                    "common.",
                    "stream.",
                    "chat.",
                    "toasts.",
                    "titlebar.",
                ]
                .iter()
                .any(|prefix| key.starts_with(prefix))
                    && !key.ends_with('.')
            }) {
                for language in ["ru", "en"] {
                    assert_ne!(
                        Strings::new(language).unwrap().text(key),
                        key,
                        "missing native label {language}:{key}"
                    );
                }
            }
        }
    }

    #[test]
    fn authentication_errors_resolve_in_both_source_catalogs() {
        for language in ["ru", "en"] {
            let strings = Strings::new(language).unwrap();
            for error in [
                crate::auth::AuthError::Network,
                crate::auth::AuthError::InvalidPassword,
                crate::auth::AuthError::Throttled(2),
                crate::auth::AuthError::CaptchaExpired,
                crate::auth::AuthError::ClientRejected,
            ] {
                let text = strings.auth_error(&error);
                assert_ne!(text, error.key());
                assert!(!text.contains("{{"));
            }
        }
    }
}
