//! Restart Morpheus (the palette's action, #232): how the new copy is
//! started. On Windows it goes through the launcher `Morpheus.exe` (so it is
//! the version installed now, an update included), with no console window,
//! and the launcher waits for this process to exit before it starts the app;
//! elsewhere a shell waits a second and starts the program.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What to run, and what it is told.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Environment variables to set.
    pub env: Vec<(String, String)>,
}

/// The variable that makes the launcher wait for a process (by id) to exit
/// before it starts the app.
pub const AFTER_PID: &str = "MORPHEUS_RESTART_AFTER_PID";

/// How to start Morpheus again from the running `exe` (with `launcher`, the
/// install's `Morpheus.exe`, if there is one) and its arguments `args`;
/// `pid` is this process.
pub fn plan(exe: &Path, launcher: Option<&Path>, args: &[String], windows: bool, pid: u32) -> Plan {
    if windows {
        return Plan {
            program: launcher.unwrap_or(exe).to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            env: vec![(AFTER_PID.to_owned(), pid.to_string())],
        };
    }
    let mut shell_args: Vec<OsString> = vec![
        "-c".into(),
        "sleep 1; exec \"$0\" \"$@\"".into(),
        exe.as_os_str().to_owned(),
    ];
    shell_args.extend(args.iter().map(OsString::from));
    Plan { program: "sh".into(), args: shell_args, env: Vec::new() }
}

/// Start the new copy; the caller quits afterwards.
pub fn spawn() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launcher = morpheus_core::install_layout::launcher_for(&exe);
    let plan = plan(&exe, launcher.as_deref(), &args, cfg!(windows), std::process::id());
    let mut command = std::process::Command::new(&plan.program);
    command.args(&plan.args);
    for (name, value) in &plan.env {
        command.env(name, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // No console window, and not tied to this process's own.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    command.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_restarts_through_the_launcher_after_this_process_ends() {
        let app = Path::new(r"C:\x\Morpheus\app\morpheus-desktop.exe");
        let launcher = Path::new(r"C:\x\Morpheus\Morpheus.exe");
        let plan = plan(app, Some(launcher), &["matrix:r/a:b".into()], true, 4242);
        assert_eq!(plan.program, launcher, "the launcher, not the app");
        assert_eq!(plan.args, [OsString::from("matrix:r/a:b")]);
        assert_eq!(plan.env, [(AFTER_PID.to_owned(), "4242".to_owned())], "it waits for the old process");
    }

    #[test]
    fn without_a_launcher_the_app_itself_is_started_waiting_the_same_way() {
        let app = Path::new(r"C:\x\Morpheus.exe");
        let plan = plan(app, None, &[], true, 7);
        assert_eq!(plan.program, app);
        assert_eq!(plan.env[0].1, "7");
    }

    #[test]
    fn elsewhere_a_shell_waits_a_second_then_starts_the_program() {
        let plan = plan(Path::new("/opt/morpheus"), None, &["a".into()], false, 1);
        assert_eq!(plan.program, Path::new("sh"));
        assert_eq!(plan.args.last(), Some(&OsString::from("a")));
        assert!(plan.env.is_empty());
    }
}

/// Whether the process `pid` is still running (the self-test only).
fn running(pid: &str) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(pid))
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

/// CI's check of Restart (`MORPHEUS_RESTART_SELF_TEST=<marker file>`; no window):
/// the first run writes a line saying who it is and restarts; the restarted
/// one (the launcher told it which process to wait for) writes a second line
/// saying who it is, that it came through the launcher, and whether the old
/// process was already gone; then both end.
pub fn self_test(marker: &Path) -> ! {
    let exe = std::env::current_exe().unwrap_or_default();
    let me = std::process::id();
    let version = crate::view_model::about::VERSION;
    let after = std::env::var(AFTER_PID).ok();
    let line = match &after {
        Some(old) => format!(
            "second pid={me} from={old} old_running={} launcher={} version={version} exe={}\n",
            running(old),
            std::env::var_os("MORPHEUS_VIA_LAUNCHER").is_some(),
            exe.display()
        ),
        None => format!("first pid={me} version={version} exe={}\n", exe.display()),
    };
    let wrote = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
    if wrote.is_err() {
        std::process::exit(1);
    }
    if after.is_none() {
        // The first run restarts; the second ends.
        if spawn().is_err() {
            std::process::exit(1);
        }
        // Gone a moment later, as a real quit is.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    std::process::exit(0);
}
