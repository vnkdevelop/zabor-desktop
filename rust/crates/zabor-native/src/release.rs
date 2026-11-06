use semver::Version;

pub const TARGET_BETA_VERSION: &str = "3.8.0-rust";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeRoute {
    NativeUpdater,
    LegacyBetaBridge,
    ExplicitBetaInstallation,
    Unsupported,
}

fn parse_version(value: &str) -> Result<Version, semver::Error> {
    let value = value.trim();
    Version::parse(
        value
            .strip_prefix('v')
            .or_else(|| value.strip_prefix('V'))
            .unwrap_or(value),
    )
}

pub fn update_allowed(
    current: &str,
    candidate: &str,
    installed_channel: Channel,
    release_channel: Channel,
) -> bool {
    let (Ok(current), Ok(candidate)) = (parse_version(current), parse_version(candidate)) else {
        return false;
    };
    installed_channel == release_channel
        && candidate.cmp_precedence(&current).is_gt()
        && (release_channel == Channel::Beta || candidate.pre.is_empty())
}

pub fn upgrade_route(current: &str, channel: Channel, native: bool) -> UpgradeRoute {
    if parse_version(current).is_err() {
        return UpgradeRoute::Unsupported;
    }
    if native {
        return UpgradeRoute::NativeUpdater;
    }
    match channel {
        Channel::Beta => UpgradeRoute::LegacyBetaBridge,
        Channel::Stable => UpgradeRoute::ExplicitBetaInstallation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_is_explicit_and_rust_prerelease_never_leaks_to_stable() {
        assert!(update_allowed(
            "3.7.2-beta.rust.2",
            TARGET_BETA_VERSION,
            Channel::Beta,
            Channel::Beta
        ));
        assert!(!update_allowed(
            "3.7.2",
            TARGET_BETA_VERSION,
            Channel::Stable,
            Channel::Beta
        ));
        assert!(!update_allowed(
            "3.7.2",
            TARGET_BETA_VERSION,
            Channel::Stable,
            Channel::Stable
        ));
        assert!(!update_allowed(
            TARGET_BETA_VERSION,
            "3.7.2-beta.rust.2",
            Channel::Beta,
            Channel::Beta
        ));
        assert!(update_allowed(
            TARGET_BETA_VERSION,
            "3.8.0-rust.1",
            Channel::Beta,
            Channel::Beta
        ));
    }

    #[test]
    fn legacy_clients_need_an_explicit_transition() {
        assert_eq!(
            upgrade_route("3.7.2", Channel::Stable, false),
            UpgradeRoute::ExplicitBetaInstallation
        );
        assert_eq!(
            upgrade_route("3.7.2-beta.rust.2", Channel::Beta, false),
            UpgradeRoute::LegacyBetaBridge
        );
        assert_eq!(
            upgrade_route(TARGET_BETA_VERSION, Channel::Beta, true),
            UpgradeRoute::NativeUpdater
        );
    }

    #[test]
    fn build_metadata_cannot_trigger_an_update_or_bypass_a_downgrade() {
        assert!(!update_allowed(
            "3.8.0-rust+one",
            "3.8.0-rust+two",
            Channel::Beta,
            Channel::Beta
        ));
        assert!(!update_allowed(
            "3.8.0-rust.10",
            "3.8.0-rust.2",
            Channel::Beta,
            Channel::Beta
        ));
        assert!(update_allowed(
            "v3.8.0-rust.2",
            " V3.8.0-rust.10 ",
            Channel::Beta,
            Channel::Beta
        ));
        assert!(!update_allowed(
            "3.8.0-rust",
            "invalid",
            Channel::Beta,
            Channel::Beta
        ));
    }
}
