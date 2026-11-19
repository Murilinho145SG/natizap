//! Updates: natizap looks for a newer release on GitHub, downloads it in
//! the background, checked against the release's checksums, and installs
//! it the way this copy was installed.
//!
//! [`Updater`] does the work and draws what the title bar shows of it:
//! nothing while natizap is current, then a button to restart into the
//! new version.

mod install;
mod release;

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, SharedString, Task, Window, div, px, rgb};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonSize, ButtonVariant};
use semver::Version;

use crate::install::{Install, Staged};
use crate::release::Release;

/// How long after natizap starts it first looks: opening comes first.
const FIRST_LOOK: Duration = Duration::from_secs(30);
/// How often it looks again while it stays open.
const LOOK_EVERY: Duration = Duration::from_hours(6);

/// Why an update could not be found or applied.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// GitHub could not be reached, or answered with an error.
    #[error("could not reach GitHub")]
    Http(#[from] ureq::Error),
    /// The release list was not what natizap reads.
    #[error("could not read the release")]
    Json(#[from] serde_json::Error),
    /// A release's tag is not a version.
    #[error("the release {0} has no version")]
    Version(String),
    /// A file the update needs is missing from the release.
    #[error("the release has no {0}")]
    Missing(String),
    /// A download does not match its checksum.
    #[error("{0} does not match its checksum")]
    Checksum(String),
    /// Writing the update failed.
    #[error("could not write the update")]
    Io(#[from] std::io::Error),
    /// The package manager or installer did not install it.
    #[error("could not install the update: {0}")]
    Install(String),
}

/// Where the update stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// natizap is current, or still looking.
    Current,
    /// A newer version is downloaded and ready.
    Ready {
        version: Version,
        staged: Staged,
        page: String,
    },
    /// The package manager is installing it.
    Installing,
    /// A newer version exists, to download by hand from `page`: this copy
    /// was not installed in a way natizap can update, or updating failed.
    Offered { version: Version, page: String },
}

/// Keeps natizap current. Draws the title bar's update button.
#[derive(Debug)]
pub struct Updater {
    install: Install,
    stage: Stage,
    _looking: Task<()>,
}

impl Updater {
    /// Starts looking for updates for the copy running from `executable`,
    /// keeping downloaded packages in `cache`.
    pub fn new(executable: &std::path::Path, cache: PathBuf, cx: &mut Context<Self>) -> Self {
        let install = Install::detect(executable);
        tracing::info!(?install, "updates");
        let looking = Self::look(install.clone(), cache, cx);
        Self {
            install,
            stage: Stage::Current,
            _looking: looking,
        }
    }

    fn look(install: Install, cache: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_LOOK).await;
            loop {
                let (install, cache, current) = (install.clone(), cache.clone(), current.clone());
                let found = cx
                    .background_spawn(async move { find(&install, &cache, &current) })
                    .await;
                let stage = match found {
                    Ok(stage) => stage,
                    Err(error) => {
                        tracing::warn!(%error, "could not look for updates");
                        None
                    }
                };
                if let Some(stage) = stage {
                    let ready = matches!(stage, Stage::Ready { .. });
                    if this.update(cx, |this, cx| this.show(stage, cx)).is_err() || ready {
                        // A downloaded update waits for the restart.
                        return;
                    }
                }
                cx.background_executor().timer(LOOK_EVERY).await;
            }
        })
    }

    fn show(&mut self, stage: Stage, cx: &mut Context<Self>) {
        self.stage = stage;
        cx.notify();
    }

    /// Restarts into the update, installing it first if it is a package,
    /// or opens the release page when it must be downloaded by hand.
    fn apply(&mut self, cx: &mut Context<Self>) {
        match self.stage.clone() {
            Stage::Ready {
                staged: Staged::Replaced(file),
                ..
            } => {
                cx.set_restart_path(file);
                cx.restart();
            }
            Stage::Ready {
                staged: Staged::Installer(file),
                version,
                page,
            } => {
                if matches!(self.install, Install::Setup) {
                    match Install::run_setup(&file) {
                        Ok(()) => cx.quit(),
                        Err(error) => self.failed(&error, version, page, cx),
                    }
                    return;
                }
                self.stage = Stage::Installing;
                cx.notify();
                let install = self.install.clone();
                cx.spawn(async move |this, cx| {
                    let installed = cx
                        .background_spawn(async move { install.install_package(&file) })
                        .await;
                    let _ = this.update(cx, |this, cx| match installed {
                        Ok(executable) => {
                            cx.set_restart_path(executable);
                            cx.restart();
                        }
                        Err(error) => this.failed(&error, version, page, cx),
                    });
                })
                .detach();
            }
            Stage::Offered { page, .. } => cx.open_url(&page),
            Stage::Current | Stage::Installing => {}
        }
    }

    fn failed(
        &mut self,
        error: &UpdateError,
        version: Version,
        page: String,
        cx: &mut Context<Self>,
    ) {
        tracing::error!(%error, "update not installed");
        self.show(Stage::Offered { version, page }, cx);
    }
}

/// The update for `install`, newer than `current`, downloaded when this
/// copy can install it; `None` when natizap is current.
fn find(
    install: &Install,
    cache: &std::path::Path,
    current: &Version,
) -> Result<Option<Stage>, UpdateError> {
    let release = Release::latest()?;
    if release.version <= *current {
        return Ok(None);
    }
    tracing::info!(version = %release.version, "a newer natizap is out");
    let offered = Stage::Offered {
        version: release.version.clone(),
        page: release.page.clone(),
    };
    match install.stage(&release, cache) {
        Ok(Some(staged)) => Ok(Some(Stage::Ready {
            version: release.version,
            staged,
            page: release.page,
        })),
        Ok(None) => Ok(Some(offered)),
        Err(error) => {
            tracing::error!(%error, "could not download the update");
            Ok(Some(offered))
        }
    }
}

impl Render for Updater {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let (note, action): (SharedString, Option<&'static str>) = match &self.stage {
            Stage::Current => return div().into_any_element(),
            Stage::Ready { version, .. } => (
                format!("natizap {version} is ready").into(),
                Some("Restart"),
            ),
            Stage::Installing => ("Installing the update…".into(), None),
            Stage::Offered { version, .. } => {
                (format!("natizap {version} is out").into(), Some("Download"))
            }
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(theme.muted))
                    .child(note),
            )
            .children(action.map(|label| {
                Button::new("update")
                    .label(label)
                    .size(ButtonSize::Small)
                    .variant(ButtonVariant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx)))
            }))
            .into_any_element()
    }
}
