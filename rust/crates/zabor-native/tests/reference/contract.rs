fn invoke(operation: &str, value: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = root.join("scripts/native/reference-contract.cjs");
    let script = script.to_str().expect("UTF-8 reference resolver path");
    assert!(!script.contains(['"', '%', '\r', '\n', '&', '|', '<', '>', '^']));
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte)));
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/v:off", "/c", "node"])
        .arg(script)
        .args([operation, value])
        .current_dir(root)
        .output()
        .expect("Node test-reference resolver must be available");
    assert!(
        output.status.success(),
        "Immutable reference admission failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 immutable test reference")
}

pub struct Snapshot {
    root: String,
    files: std::collections::BTreeMap<String, String>,
    catalogs: std::collections::BTreeMap<String, serde_json::Value>,
}

impl Snapshot {
    pub fn admit(group: &str) -> Self {
        Self::admit_with(group, invoke)
    }

    fn admit_with(group: &str, admission: impl FnOnce(&str, &str) -> String) -> Self {
        let payload = admission("--snapshot", group);
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let root = value["root"].as_str().unwrap().to_owned();
        let files: std::collections::BTreeMap<String, String> =
            serde_json::from_value(value["files"].clone()).unwrap();
        let mut snapshot = Self {
            root,
            files,
            catalogs: std::collections::BTreeMap::new(),
        };
        assert!(!snapshot.root().is_empty());
        for language in ["ru", "en"] {
            let source = snapshot.text(&format!("src/renderer/src/locales/{language}.json"));
            let catalog = serde_json::from_str(source).unwrap();
            snapshot.catalogs.insert(language.to_owned(), catalog);
        }
        snapshot
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn text(&self, file: &str) -> &str {
        &self.files[file]
    }

    pub fn catalog(&self, language: &str) -> &serde_json::Value {
        &self.catalogs[language]
    }
}
