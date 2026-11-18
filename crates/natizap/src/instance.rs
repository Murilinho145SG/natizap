//! One natizap at a time: two would both speak for the same linked device
//! and write the same local stores. A later launch asks the running one to
//! show itself, then leaves.
//!
//! The running one holds a lock on a file in the data directory, released
//! by the system when it exits, crashes included, and listens on a local
//! port it writes beside it.

use std::fs::{File, TryLockError};
use std::io::{self, BufRead as _, BufReader, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};

const SHOW: &str = "show";

/// Who got to run.
pub(crate) enum Claim {
    /// This launch runs natizap.
    First(Instance),
    /// Another natizap already runs.
    Running,
}

/// The running natizap's hold on the data directory.
pub(crate) struct Instance {
    lock: File,
    listener: TcpListener,
}

impl Instance {
    /// Claims `dir` for this launch, unless another natizap holds it.
    ///
    /// # Errors
    ///
    /// Fails when the lock or port files cannot be written, or no local
    /// port can be opened.
    pub(crate) fn claim(dir: &Path) -> io::Result<Claim> {
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("natizap.lock"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(Claim::Running),
            Err(TryLockError::Error(error)) => return Err(error),
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        std::fs::write(port_file(dir), listener.local_addr()?.port().to_string())?;
        Ok(Claim::First(Self { lock, listener }))
    }

    /// Hears later launches asking to be shown, one message each, from a
    /// thread of its own; the hold lasts as long as the process.
    pub(crate) fn listen(self) -> async_channel::Receiver<()> {
        let (asks, asked) = async_channel::unbounded();
        let Self { lock, listener } = self;
        std::thread::Builder::new()
            .name("natizap-instance".into())
            .spawn(move || {
                let _lock = lock;
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let mut line = String::new();
                    if BufReader::new(stream).read_line(&mut line).is_ok()
                        && line.trim() == SHOW
                        && asks.send_blocking(()).is_err()
                    {
                        break;
                    }
                }
            })
            .map_or_else(
                |error| tracing::error!(%error, "cannot hear later launches"),
                drop,
            );
        asked
    }
}

/// Asks the natizap running from `dir` to show itself.
///
/// # Errors
///
/// Fails when its port cannot be read or it does not answer.
pub(crate) fn ask_to_show(dir: &Path) -> io::Result<()> {
    let port: u16 = std::fs::read_to_string(port_file(dir))?
        .trim()
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    writeln!(stream, "{SHOW}")
}

fn port_file(dir: &Path) -> PathBuf {
    dir.join("natizap.port")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_launch_finds_the_first_and_asks_it_to_show() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("natizap-instance-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let Claim::First(first) = Instance::claim(&dir)? else {
            panic!("the first launch must run");
        };
        assert!(matches!(Instance::claim(&dir)?, Claim::Running));
        let asked = first.listen();
        ask_to_show(&dir)?;
        assert_eq!(asked.recv_blocking(), Ok(()));
        std::fs::remove_dir_all(&dir)
    }
}
