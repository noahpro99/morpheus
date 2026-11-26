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
            Err(err) => eprintln!("morpheus-desktop: single-instance socket not bound: {err}"),
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

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::{path::Path, sync::OnceLock};

    use windows::{
        Win32::{
            Foundation::{ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
            System::Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
        },
        core::PCWSTR,
    };

    use crate::view_model::instance::Request;

    /// The pipe's handle, held for the whole run (its first instance is what
    /// says Morpheus runs on this data directory).
    static PIPE: OnceLock<isize> = OnceLock::new();

    fn pipe_name(data_dir: &Path) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.hash(&mut hasher);
        format!(r"\\.\pipe\morpheus-{:016x}-instance", hasher.finish())
    }

    fn tell_running(name: &str, link: Option<&str>) -> bool {
        use std::io::Write;
        let Ok(mut running) = std::fs::OpenOptions::new().write(true).open(name) else {
            return false;
        };
        let request = match link {
            Some(link) => Request::Open(link.to_owned()),
            None => Request::Show,
        };
        let _ = running.write_all(request.line().as_bytes());
        true
    }

    /// `false` when another Morpheus already runs on `data_dir`; it has been
    /// asked to show itself (or open `link`), and this process should exit.
    pub fn claim(data_dir: &Path, link: Option<&str>) -> bool {
        let name = pipe_name(data_dir);
        if tell_running(&name, link) {
            return false;
        }
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is a NUL-terminated wide string that outlives the call.
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            // Someone else has it (a launch that raced this one, or the server
            // between two clients): ask it again a few times.
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if tell_running(&name, link) {
                    return false;
                }
            }
            eprintln!("morpheus-desktop: single-instance pipe not made");
            return true;
        }
        let _ = PIPE.set(handle.0 as isize);
        true
    }

    /// Call `handle` for every later launch that asks.
    pub fn listen(handle: impl Fn(Request) + Send + 'static) {
        let Some(&raw) = PIPE.get() else {
            return;
        };
        std::thread::spawn(move || {
            use std::io::BufRead;
            use std::os::windows::io::{FromRawHandle as _, RawHandle};
            let pipe = HANDLE(raw as *mut _);
            loop {
                // SAFETY: a valid pipe handle owned by this thread from here on.
                let connected = unsafe { ConnectNamedPipe(pipe, None) };
                let ok = match connected {
                    Ok(()) => true,
                    Err(err) => err.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                };
                if ok {
                    // SAFETY: the same handle, borrowed as a File that is never dropped
                    // (the pipe lives for the run).
                    let file = std::mem::ManuallyDrop::new(unsafe {
                        std::fs::File::from_raw_handle(raw as RawHandle)
                    });
                    let mut line = String::new();
                    let _ = std::io::BufReader::new(&*file).read_line(&mut line);
                    handle(Request::parse(&line));
                }
                // SAFETY: as above; frees the instance for the next client.
                let _ = unsafe { DisconnectNamedPipe(pipe) };
            }
        });
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::path::Path;

    use crate::view_model::instance::Request;

    pub fn claim(_: &Path, _: Option<&str>) -> bool {
        true
    }

    pub fn listen(_: impl Fn(Request) + Send + 'static) {}
}

pub use imp::{claim, listen};
