use clap::Parser;
use homedir::my_home;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value_t = 0)]
    pub nox: u8,
    #[arg(short, long, default_value = "")]
    pub save: String,
    #[arg(short, long, default_value = "")]
    pub delete: String,
    #[arg(short, long, default_value = "")]
    pub migrate: String,
    #[arg(short = 'y', long, default_value = "")]
    pub key: String,
    #[arg(short = 'k', long, default_value = "")]
    pub value: String,
    #[arg(short = 'v', long, default_value = "")]
    pub master: String,
    #[arg(short = 'r', long, default_value = "")]
    pub db: String,
    #[arg(short = 'b', long, default_value = "")]
    pub repl: String,
    #[arg(short = 'q', long, default_value = "")]
    pub query: String,
    #[arg(short = 't', long, default_value = "")]
    pub table: String,
}

pub fn get_config() -> AppConfig {
    let config = match load_or_initialize() {
        Ok(v) => v,
        Err(err) => {
            match err {
                ConfigError::IoError(err) => {
                    eprintln!("An error occurred while loading the config: {err}");
                }
                ConfigError::InvalidConfig(err) => {
                    eprintln!("An error occurred while parsing the config:");
                    eprintln!("{err}");
                }
            }
            AppConfig::default()
        }
    };
    //println!("{:?}", config);
    return config;
    //    return "xxxx".to_string();
}

enum ConfigError {
    IoError(io::Error),
    InvalidConfig(toml::de::Error),
}

impl From<io::Error> for ConfigError {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        Self::InvalidConfig(value)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub host: String,
    pub user: String,
    pub database: String,
    pub password: String,
    #[serde(default)]
    pub mysql_host: String,
    #[serde(default)]
    pub mysql_user: String,
    #[serde(default)]
    pub mysql_password: String,
    #[serde(default)]
    pub mysql_database: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            user: "".to_string(),
            password: "".to_string(),
            database: "".to_string(),
            mysql_host: "127.0.0.1".to_string(),
            mysql_user: "root".to_string(),
            mysql_password: "".to_string(),
            mysql_database: "".to_string(),
        }
    }
}

fn load_or_initialize() -> Result<AppConfig, ConfigError> {
    let home = my_home()
        .ok()
        .flatten()
        .ok_or_else(|| ConfigError::IoError(io::Error::new(io::ErrorKind::NotFound, "Cannot determine home directory")))?;
    let config_path = home.join("config.toml");
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let config = toml::from_str(&content)?;
        return Ok(config);
    }

    let config = AppConfig::default();
    let toml = toml::to_string(&config).unwrap();

    fs::write(&config_path, toml)?;
    Ok(config)
}
