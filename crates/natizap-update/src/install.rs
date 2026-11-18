//! How this copy of natizap was installed, which decides how an update
//! reaches it: an AppImage is replaced in place, a system package goes
//! through the package manager, Windows through its installer.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::UpdateError;
use crate::release::Release;

/// Where a package says which kind it is, relative to the executable's
/// directory: each package installs it with its own kind written in it.
const PACKAGE_MARK: &str = "../lib/natizap/install-kind";

/// How this copy was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Install {
    /// A single AppImage file.
    AppImage(PathBuf),
    /// A system package, and the executable it installed.
    Package(Package, PathBuf),
    /// Installed on Windows with natizap's installer.
    Setup,
    /// Built or unpacked by hand, or on a platform without release builds:
    /// only the release page is offered.
    Manual,
}

/// A system package format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Package {
    Deb,
    Rpm,
    Pacman,
}

/// An update downloaded and ready to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Staged {
    /// Already in place: restarting runs it.
    Replaced(PathBuf),
    /// A package or installer to run.
    Installer(PathBuf),
}

impl Install {
    /// How the copy running from `executable` was installed.
    pub(crate) fn detect(executable: &Path) -> Self {
        // Releases are built for 64-bit x86 only.
        if std::env::consts::ARCH != "x86_64" {
            return Self::Manual;
        }
        if cfg!(windows) {
            // The installer leaves its uninstaller beside the executable.
            let installed = executable
                .parent()
                .is_some_and(|dir| dir.join("unins000.exe").exists());
            return if installed { Self::Setup } else { Self::Manual };
        }
        // The AppImage runtime names the file it runs from.
        if let Some(image) = std::env::var_os("APPIMAGE") {
            return Self::AppImage(PathBuf::from(image));
        }
        let kind = executable
            .parent()
            .and_then(|dir| std::fs::read_to_string(dir.join(PACKAGE_MARK)).ok());
        match kind.as_deref().map(str::trim) {
            Some("deb") => Self::Package(Package::Deb, executable.to_owned()),
            Some("rpm") => Self::Package(Package::Rpm, executable.to_owned()),
            Some("pacman") => Self::Package(Package::Pacman, executable.to_owned()),
            _ => Self::Manual,
        }
    }

    /// The end of the name of the release file this copy updates from.
    fn suffix(&self) -> Option<&'static str> {
        match self {
            Self::AppImage(_) => Some("-x86_64.AppImage"),
            Self::Package(Package::Deb, _) => Some("_amd64.deb"),
            Self::Package(Package::Rpm, _) => Some(".x86_64.rpm"),
            Self::Package(Package::Pacman, _) => Some("-x86_64.pkg.tar.zst"),
            Self::Setup => Some("-x86_64-setup.exe"),
            Self::Manual => None,
        }
    }

    /// Downloads `release` for this copy: an AppImage straight over the
    /// running one, which keeps running from memory; a package or
    /// installer into `cache`. `None` for a copy updated by hand.
    pub(crate) fn stage(
        &self,
        release: &Release,
        cache: &Path,
    ) -> Result<Option<Staged>, UpdateError> {
        let Some(suffix) = self.suffix() else {
            return Ok(None);
        };
        let asset = release
            .asset(suffix)
            .ok_or_else(|| UpdateError::Missing(suffix.to_owned()))?;
        if let Self::AppImage(image) = self {
            // Beside it, so the rename over it stays on one file system.
            let part = image.with_extension("AppImage.part");
            release.download(asset, &part)?;
            make_executable(&part)?;
            std::fs::rename(&part, image)?;
            return Ok(Some(Staged::Replaced(image.clone())));
        }
        std::fs::create_dir_all(cache)?;
        let file = cache.join(&asset.name);
        release.download(asset, &file)?;
        Ok(Some(Staged::Installer(file)))
    }

    /// Installs `file`, a staged package, asking for the administrator's
    /// password through polkit, and waits for it. Returns the executable to
    /// start again.
    pub(crate) fn install_package(&self, file: &Path) -> Result<PathBuf, UpdateError> {
        let Self::Package(package, executable) = self else {
            return Err(UpdateError::Install("not a package install".into()));
        };
        let mut command = Command::new("pkexec");
        match package {
            // apt also installs dependencies a new version gained.
            Package::Deb => command.args(["apt-get", "install", "-y"]),
            Package::Rpm => command.args(["rpm", "-U", "--replacepkgs"]),
            Package::Pacman => command.args(["pacman", "-U", "--noconfirm"]),
        };
        let status = command.arg(file).status()?;
        if status.success() {
            Ok(executable.clone())
        } else {
            Err(UpdateError::Install(format!(
                "the package manager exited with {status}"
            )))
        }
    }

    /// Starts the Windows installer `file` without its wizard; it closes
    /// natizap, updates it and starts it again.
    pub(crate) fn run_setup(file: &Path) -> Result<(), UpdateError> {
        Command::new(file)
            .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .spawn()?;
        Ok(())
    }
}

#[cfg(unix)]
fn make_executable(file: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_is_told_by_the_mark_it_installs() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("natizap-install-{}", std::process::id()));
        std::fs::create_dir_all(root.join("usr/bin"))?;
        std::fs::create_dir_all(root.join("usr/lib/natizap"))?;
        let executable = root.join("usr/bin/natizap");
        let unmarked = Install::detect(&executable);
        std::fs::write(root.join("usr/lib/natizap/install-kind"), "rpm\n")?;
        let marked = Install::detect(&executable);
        std::fs::remove_dir_all(&root)?;
        if std::env::var_os("APPIMAGE").is_none() && cfg!(not(windows)) {
            assert_eq!(unmarked, Install::Manual);
            assert_eq!(marked, Install::Package(Package::Rpm, executable));
        }
        Ok(())
    }
}
