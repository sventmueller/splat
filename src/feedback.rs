//! The way from the app to the project on GitHub.

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSProcessInfo, NSString, NSURL};

use crate::util::ns;

const REPOSITORY: &str = "https://github.com/sventmueller/splat";

fn open(address: &str) {
    if let Some(url) = NSURL::URLWithString(&ns(address)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

pub fn report_problem() {
    open(&problem_form());
}

pub fn suggest_feature() {
    open(&format!("{REPOSITORY}/discussions/new?category=ideas"));
}

pub fn show_repository() {
    open(REPOSITORY);
}

/// The problem form with what Splat knows about itself filled in.
pub fn problem_form() -> String {
    let version = NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&ns("CFBundleShortVersionString"))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|v| v.to_string())
        .unwrap_or_default();
    let system = NSProcessInfo::processInfo().operatingSystemVersion();
    let macos = format!(
        "{}.{}.{}",
        system.majorVersion, system.minorVersion, system.patchVersion
    );
    problem_address(&version, &macos, cfg!(target_arch = "aarch64"))
}

/// GitHub fills the fields of `.github/ISSUE_TEMPLATE/bug.yml` from the query by their ids.
fn problem_address(version: &str, macos: &str, apple_silicon: bool) -> String {
    let mac = if apple_silicon { "Apple%20Silicon" } else { "Intel" };
    format!("{REPOSITORY}/issues/new?template=bug.yml&version={version}&macos={macos}&mac={mac}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_problem_form_is_filled_in_by_the_ids_of_its_fields() {
        assert_eq!(
            problem_address("1.0.9", "26.1.0", true),
            "https://github.com/sventmueller/splat/issues/new?template=bug.yml&version=1.0.9&macos=26.1.0&mac=Apple%20Silicon"
        );
        assert!(problem_address("1.0.9", "26.1.0", false).ends_with("&mac=Intel"));
        let form = include_str!("../.github/ISSUE_TEMPLATE/bug.yml");
        for id in ["id: version", "id: macos", "id: mac"] {
            assert!(form.contains(id), "{id}");
        }
    }
}
