use crate::error::{Error, Result};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub vnc_port: u16,
    pub vnc_password: String,
    pub access_token: String,
    pub ffmpeg: PathBuf,
    #[serde(default = "default_port")]
    pub listen_port: u16,
    pub public_origin: String,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_bitrate")]
    pub bitrate_kbps: u32,
}
fn default_port() -> u16 {
    8787
}
fn default_fps() -> u32 {
    30
}
fn default_bitrate() -> u32 {
    6000
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.mode() & 0o077 != 0 {
                return Err(Error::Config("configuration must be owner-only"));
            }
        }
        if meta.len() > 16 * 1024 {
            return Err(Error::Limit("configuration size"));
        }
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|_| Error::Config("invalid configuration file"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.vnc_port == 0 || self.listen_port == 0 {
            return Err(Error::Config("port"));
        }
        if self.vnc_password.is_empty() || self.access_token.len() < 32 {
            return Err(Error::Config("credentials"));
        }
        if !(1..=60).contains(&self.fps) || !(100..=50000).contains(&self.bitrate_kbps) {
            return Err(Error::Config("encoder settings"));
        }
        if !self.ffmpeg.is_absolute() {
            return Err(Error::Config("FFmpeg path must be absolute"));
        }
        let uri: axum::http::Uri = self
            .public_origin
            .parse()
            .map_err(|_| Error::Config("public origin"))?;
        if uri.authority().is_none()
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        {
            return Err(Error::Config("public origin"));
        }
        let local = matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if uri.scheme_str() != Some("https") && !(local && uri.scheme_str() == Some("http")) {
            return Err(Error::Config("HTTPS required"));
        }
        if self.public_origin.ends_with('/') {
            return Err(Error::Config("origin must not have trailing slash"));
        }
        Ok(())
    }
    pub fn read_timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
}
