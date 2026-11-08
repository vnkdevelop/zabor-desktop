use semver::Version;

pub const TARGET_BETA_VERSION: &str = "3.8.0-rust";
pub const NATIVE_BETA_PROFILE: &str = "zabor-desktop-native-beta";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReleaseGate {
    pub complete_native_acceptance: bool,
    pub windows_10_11_x64_acceptance: bool,
    pub two_device_acceptance: bool,
    pub data_compatibility_acceptance: bool,
    pub package_composition_verified: bool,
    pub license_inventory_verified: bool,
    pub manifest_key_pinned: bool,
    pub publisher_certificate_pinned: bool,
}

impl ReleaseGate {
    pub fn installation_allowed(self) -> bool {
        self.complete_native_acceptance
            && self.windows_10_11_x64_acceptance
            && self.two_device_acceptance
            && self.data_compatibility_acceptance
            && self.package_composition_verified
            && self.license_inventory_verified
            && self.manifest_key_pinned
            && self.publisher_certificate_pinned
    }
}

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
    let _ = channel;
    UpgradeRoute::ExplicitBetaInstallation
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
            UpgradeRoute::ExplicitBetaInstallation
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

    #[test]
    fn release_gate_is_closed_without_complete_acceptance_and_both_pins() {
        assert!(!ReleaseGate::default().installation_allowed());
        let local_checks = ReleaseGate {
            package_composition_verified: true,
            license_inventory_verified: true,
            ..ReleaseGate::default()
        };
        assert!(!local_checks.installation_allowed());
        assert_eq!(NATIVE_BETA_PROFILE, "zabor-desktop-native-beta");
        assert_eq!(
            upgrade_route("invalid", Channel::Beta, false),
            UpgradeRoute::Unsupported
        );
    }
}
