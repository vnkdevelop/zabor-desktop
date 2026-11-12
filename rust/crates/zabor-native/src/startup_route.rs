use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupRoute {
    Help,
    Preview { updater: bool },
    App,
    Developer,
    Health,
    Baseline { bundle: PathBuf, archive: PathBuf },
}

impl StartupRoute {
    pub fn parse(arguments: &[String]) -> Result<Self, &'static str> {
        if arguments == ["--help"] {
            return Ok(Self::Help);
        }
        if arguments == ["--native-update-health"] {
            return Ok(Self::Health);
        }
        if arguments
            .first()
            .is_some_and(|arg| arg == "--native-establish-baseline")
        {
            return match arguments {
                [_, bundle_option, bundle, archive_option, archive]
                    if bundle_option == "--bundle"
                        && archive_option == "--archive"
                        && !bundle.is_empty()
                        && !archive.is_empty() =>
                {
                    Ok(Self::Baseline {
                        bundle: bundle.into(),
                        archive: archive.into(),
                    })
                }
                _ => Err("baseline-exact-arguments-required"),
            };
        }
        let mut seen = BTreeSet::new();
        let mut preview = None;
        let mut snapshot = false;
        let mut interaction = false;
        let mut app = false;
        let mut developer = false;
        let mut index = 0;
        while index < arguments.len() {
            let option = arguments[index].as_str();
            if !seen.insert(option) {
                return Err("duplicate-startup-option");
            }
            match option {
                "--app" => app = true,
                "--developer" => developer = true,
                "--interaction-check" => interaction = true,
                "--reduced-motion" => {}
                "--locale" | "--server" | "--profile" | "--snapshot" | "--preview" | "--scale"
                | "--at-ms" | "--state" => {
                    index += 1;
                    let value = arguments
                        .get(index)
                        .filter(|value| !value.is_empty() && !value.starts_with("--"))
                        .ok_or("missing-startup-option-value")?;
                    if option == "--preview" {
                        preview = Some(value == "updater");
                    }
                    if option == "--snapshot" {
                        snapshot = true;
                    }
                }
                _ => return Err("unsupported-startup-option"),
            }
            index += 1;
        }
        if app && (preview.is_some() || snapshot || interaction) {
            return Err("app-preview-routes-cannot-mix");
        }
        if snapshot && interaction {
            return Err("snapshot-interaction-routes-cannot-mix");
        }
        if developer {
            if app
                || preview.is_some()
                || snapshot
                || interaction
                || seen.contains("--profile")
                || seen.contains("--state")
                || seen.contains("--at-ms")
            {
                return Err("developer-route-cannot-mix");
            }
            return Ok(Self::Developer);
        }
        if preview.is_some() || snapshot || interaction {
            Ok(Self::Preview {
                updater: preview == Some(true),
            })
        } else if seen.contains("--profile") || seen.contains("--state") || seen.contains("--at-ms")
        {
            Err("live-profile-override-or-preview-options-rejected")
        } else {
            Ok(Self::App)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(input: &[&str]) -> Vec<String> {
        input.iter().map(|arg| (*arg).into()).collect()
    }

    #[test]
    fn startup_routes_reject_health_smuggling_duplicates_and_missing_values() {
        assert_eq!(
            StartupRoute::parse(&args(&["--native-update-health"])),
            Ok(StartupRoute::Health)
        );
        for input in [
            vec!["--app", "--native-update-health"],
            vec!["--native-update-health", "--app"],
            vec!["--native-update-health", "--locale", "en"],
            vec!["--help", "--app"],
            vec!["--app", "--app"],
            vec!["--locale"],
            vec!["--server", "--app"],
            vec!["--app", "--preview", "home"],
            vec!["--profile", "other"],
            vec!["--state", "error"],
        ] {
            assert!(StartupRoute::parse(&args(&input)).is_err(), "{input:?}");
        }
    }

    #[test]
    fn developer_route_is_explicit_and_cannot_mix_with_other_routes() {
        assert_eq!(
            StartupRoute::parse(&args(&["--developer"])),
            Ok(StartupRoute::Developer)
        );
        assert_eq!(
            StartupRoute::parse(&args(&[
                "--developer",
                "--locale",
                "en",
                "--server",
                "http://127.0.0.1:8080/zabor_v3"
            ])),
            Ok(StartupRoute::Developer)
        );
        for input in [
            vec!["--developer", "--app"],
            vec!["--developer", "--preview", "home"],
            vec!["--developer", "--snapshot", "a.png"],
            vec!["--developer", "--interaction-check"],
            vec!["--developer", "--profile", "other"],
            vec!["--developer", "--developer"],
        ] {
            assert!(StartupRoute::parse(&args(&input)).is_err(), "{input:?}");
        }
    }

    #[test]
    fn startup_routes_keep_developer_previews_and_explicit_baseline_distinct() {
        assert_eq!(StartupRoute::parse(&[]), Ok(StartupRoute::App));
        assert_eq!(
            StartupRoute::parse(&args(&["--preview", "updater", "--state", "error"])),
            Ok(StartupRoute::Preview { updater: true })
        );
        assert_eq!(
            StartupRoute::parse(&args(&["--preview", "home", "--snapshot", "local.png"])),
            Ok(StartupRoute::Preview { updater: false })
        );
        assert!(matches!(
            StartupRoute::parse(&args(&[
                "--native-establish-baseline",
                "--bundle",
                "signed.json",
                "--archive",
                "package.zip"
            ])),
            Ok(StartupRoute::Baseline { .. })
        ));
        assert!(StartupRoute::parse(&args(&["--native-establish-baseline"])).is_err());
    }
}
