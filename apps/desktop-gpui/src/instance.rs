//! One Morpheus per data directory (`single_instance` in
//! `apps/desktop/src/main.rs`): a second launch asks the running one over a
//! socket in the runtime directory to show its window (or open the Matrix
//! link it was started with), then exits.


#[cfg(unix)]
mod imp {
    use std::{
        os::unix::net::{UnixListener, UnixStream},
        path::{Path, PathBuf},
        sync::OnceLock,
    };

    use crate::view_model::instance::Request;

    static LISTENER: OnceLock<UnixListener> = OnceLock::new();

    /// The socket for `data_dir`: in the runtime dir, named after a hash of
    /// the data dir (socket paths are limited to about a hundred bytes).
    fn socket_path(data_dir: &Path) -> PathBuf {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.hash(&mut hasher);
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        dir.join(format!("morpheus-{:016x}-instance.sock", hasher.finish()))
    }

    /// `false` when another Morpheus already runs on `data_dir`; it has been
    /// asked to show itself (or open `link`), and this process should exit.
    pub fn claim(data_dir: &Path, link: Option<&str>) -> bool {
        use std::io::Write;
        let path = socket_path(data_dir);
        if let Ok(mut running) = UnixStream::connect(&path) {
            let request = match link {
                Some(link) => Request::Open(link.to_owned()),
                None => Request::Show,
            };
            let _ = running.write_all(request.line().as_bytes());
            return false;
        }
        // Nobody answered: a leftover from a crash, or nothing yet.
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let _ = LISTENER.set(listener);
            }
            Err(err) => eprintln!("morpheus-gpui: single-instance socket not bound: {err}"),
        }
        true
    }

    /// Call `handle` for every later launch that asks.
    pub fn listen(handle: impl Fn(Request) + Send + 'static) {
        let Some(listener) = LISTENER.get() else {
            return;
        };
        std::thread::spawn(move || {
            use std::io::BufRead;
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = std::io::BufReader::new(stream).read_line(&mut line);
                handle(Request::parse(&line));
            }
        });
    }
}

#[cfg(not(unix))]
mod imp {
    use std::path::Path;

    use crate::view_model::instance::Request;

    pub fn claim(_: &Path, _: Option<&str>) -> bool {
        true
    }

    pub fn listen(_: impl Fn(Request) + Send + 'static) {}
}

pub use imp::{claim, listen};


