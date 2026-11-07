//! Bug reports: a link to a new GitHub issue in the bug template, filled
//! in with this version, the device, and the end of this run's log. The
//! person sees and edits all of it before posting (issues are public).

use std::path::Path;

/// Where issues are filed.
const NEW_ISSUE: &str = "https://github.com/noahpro99/morpheus/issues/new";

/// At most this much of the log goes in: GitHub refuses links much longer
/// than 8 KB, and the log grows several times over once escaped.
const LOG_CHARS: usize = 2500;

/// The new-issue link, for `version` (the app's) with the log in `data_dir`.
pub fn bug_report_url(version: &str, data_dir: &Path) -> String {
    let logs = std::fs::read_to_string(data_dir.join("morpheus.log"))
        .map(|log| log_tail(&log, LOG_CHARS))
        .unwrap_or_default();
    let mut url = url::Url::parse(NEW_ISSUE).unwrap_or_else(|_| unreachable!("a fixed url"));
    url.query_pairs_mut()
        .append_pair("template", "bug_report.yml")
        .append_pair("version", version)
        .append_pair("device", &device())
        .append_pair("logs", &logs);
    url.into()
}

/// The last whole lines of `log` that fit in `max` bytes.
fn log_tail(log: &str, max: usize) -> String {
    let mut kept = Vec::new();
    let mut size = 0;
    for line in log.trim_end().lines().rev() {
        size += line.len() + 1;
        if size > max {
            break;
        }
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n")
}

/// The system and processor, e.g. "Linux (Ubuntu 24.04 LTS) x86_64",
/// "macOS 26.6 aarch64", "Android 16 (Pixel 9) aarch64".
pub fn device() -> String {
    let arch = std::env::consts::ARCH;
    let system = match std::env::consts::OS {
        "linux" => {
            let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
            let name = release
                .lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|n| n.trim_matches('"').to_owned());
            match name {
                Some(name) => format!("Linux ({name})"),
                None => "Linux".to_owned(),
            }
        }
        "macos" => match command("sw_vers", &["-productVersion"]) {
            Some(version) => format!("macOS {version}"),
            None => "macOS".to_owned(),
        },
        "android" => {
            let version = command("getprop", &["ro.build.version.release"]);
            let model = command("getprop", &["ro.product.model"]);
            match (version, model) {
                (Some(v), Some(m)) => format!("Android {v} ({m})"),
                (Some(v), None) => format!("Android {v}"),
                _ => "Android".to_owned(),
            }
        }
        "ios" => "iOS".to_owned(),
        "windows" => "Windows".to_owned(),
        other => other.to_owned(),
    };
    format!("{system} {arch}")
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::log_tail;

    #[test]
    fn the_tail_is_whole_lines_within_the_limit() {
        let log = "first line\nsecond line\nthird\n";
        assert_eq!(log_tail(log, 18), "second line\nthird");
        assert_eq!(log_tail(log, 4), "");
    }
}
