
use std::{fs::read_to_string, string::String};

use anyhow::{Context, Result};
use log::debug;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Exec {
    String(String),
    List(Vec<String>),
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Input {
    pub port: String,
    pub note: String,
    pub exec: Exec,
    pub hold_time: Option<String>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct Config {
    pub inputs: Vec<Input>,
}

pub fn get_config() -> Result<Config> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("katarl")
        .context("failed to read app directories")?;
    let conf_path = xdg_dirs.place_config_file("config.toml")
        .context("failed to find config path")?;
    debug!("loading configuration from {}", conf_path.display());
    let file = read_to_string(&conf_path)
        .context(format!("failed to read config {}", conf_path.display()))?;
    let config = toml::from_str::<Config>(&file)?;
    Ok(config)
}
