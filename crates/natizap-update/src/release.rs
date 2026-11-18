//! natizap's releases on GitHub: which one is the newest, and fetching one
//! of its files, checked against the checksums published beside it.

use std::fs::File;
use std::io::{BufWriter, Read as _, Write as _};
use std::path::Path;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::UpdateError;

/// The file listing each release file's SHA-256, as `sha256sum` writes it.
const CHECKSUMS: &str = "SHA256SUMS";

/// GitHub's API refuses requests that do not name their client.
const USER_AGENT: &str = "natizap-updater";

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Release {
    pub(crate) version: Version,
    /// The release's page, to download it by hand.
    pub(crate) page: String,
    assets: Vec<Asset>,
}

/// A file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Asset {
    pub(crate) name: String,
    #[serde(rename = "browser_download_url")]
    url: String,
}

/// The fields of GitHub's release JSON natizap reads.
#[derive(Debug, Deserialize)]
struct Published {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

impl Release {
    /// The newest release, asked of GitHub.
    pub(crate) fn latest() -> Result<Self, UpdateError> {
        // The repository named in the workspace is where releases go.
        let api = env!("CARGO_PKG_REPOSITORY").replacen(
            "https://github.com/",
            "https://api.github.com/repos/",
            1,
        );
        let body = ureq::get(format!("{api}/releases/latest"))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .call()?
            .body_mut()
            .read_to_string()?;
        Self::parse(&body)
    }

    fn parse(json: &str) -> Result<Self, UpdateError> {
        let published: Published = serde_json::from_str(json)?;
        let version = published.tag_name.trim_start_matches('v');
        Ok(Self {
            version: Version::parse(version)
                .map_err(|_| UpdateError::Version(published.tag_name.clone()))?,
            page: published.html_url,
            assets: published.assets,
        })
    }

    /// The file whose name ends with `suffix`, e.g. `_amd64.deb`.
    pub(crate) fn asset(&self, suffix: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|asset| asset.name.ends_with(suffix))
    }

    /// Downloads `asset` to `destination`, checking it against the
    /// release's checksums; a file that does not match is removed.
    pub(crate) fn download(&self, asset: &Asset, destination: &Path) -> Result<(), UpdateError> {
        let expected = self.checksum(&asset.name)?;
        let mut response = ureq::get(&asset.url)
            .header("User-Agent", USER_AGENT)
            .call()?;
        let mut reader = response.body_mut().as_reader();
        let mut file = BufWriter::new(File::create(destination)?);
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])?;
        }
        file.flush()?;
        if hex(&hasher.finalize()) != expected {
            std::fs::remove_file(destination)?;
            return Err(UpdateError::Checksum(asset.name.clone()));
        }
        Ok(())
    }

    /// The SHA-256 the release lists for the file `name`, in hex.
    fn checksum(&self, name: &str) -> Result<String, UpdateError> {
        let list = self
            .asset(CHECKSUMS)
            .ok_or_else(|| UpdateError::Missing(CHECKSUMS.to_owned()))?;
        let sums = ureq::get(&list.url)
            .header("User-Agent", USER_AGENT)
            .call()?
            .body_mut()
            .read_to_string()?;
        checksum_in(&sums, name).ok_or_else(|| UpdateError::Missing(format!("{CHECKSUMS}: {name}")))
    }
}

/// The checksum `sums`, a `sha256sum` listing, gives for `name`.
fn checksum_in(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        // `sha256sum` marks files read in binary mode with a star.
        (file.trim_start().trim_start_matches('*') == name).then(|| sum.to_ascii_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_and_files_of_a_release() {
        let json = r#"{
            "tag_name": "v0.2.1",
            "html_url": "https://github.com/o/r/releases/tag/v0.2.1",
            "assets": [
                {"name": "natizap_0.2.1_amd64.deb", "browser_download_url": "https://x/deb"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/sums"}
            ]
        }"#;
        let release = Release::parse(json);
        assert_eq!(
            release
                .as_ref()
                .ok()
                .map(|release| (release.version.to_string(), release.asset("_amd64.deb"))),
            Some((
                "0.2.1".to_owned(),
                Some(&Asset {
                    name: "natizap_0.2.1_amd64.deb".into(),
                    url: "https://x/deb".into(),
                })
            ))
        );
    }

    #[test]
    fn finds_a_files_checksum_in_a_sha256sum_listing() {
        let sums = "AB12  natizap.AppImage\ncd34 *natizap-setup.exe\n";
        assert_eq!(
            checksum_in(sums, "natizap-setup.exe").as_deref(),
            Some("cd34")
        );
        assert_eq!(
            checksum_in(sums, "natizap.AppImage").as_deref(),
            Some("ab12")
        );
        assert_eq!(checksum_in(sums, "other"), None);
    }
}
