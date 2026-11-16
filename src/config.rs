use clap::Parser;
use homedir::my_home;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

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
    #[arg(short = 'a', long, default_value = "")]
    pub action: String,
    #[arg(long, default_value = "")]
    pub roles: String,
    #[arg(long, default_value_t = false)]
    pub readonly: bool,
    #[arg(long, default_value_t = false)]
    pub yes: bool,
}

pub fn get_config() -> AppConfig {
    match load_or_initialize() {
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
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), ConfigError> {
    let config_path = config_path()?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let toml = toml::to_string_pretty(config).unwrap();
    fs::write(config_path, toml)?;
    Ok(())
}

fn config_path() -> Result<PathBuf, ConfigError> {
    let home = my_home().ok().flatten().ok_or_else(|| {
        ConfigError::IoError(io::Error::new(
            io::ErrorKind::NotFound,
            "Cannot determine home directory",
        ))
    })?;
    Ok(home.join("config.toml"))
}

#[derive(Debug)]
pub enum ConfigError {
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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ServerConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub database: String,
}

impl ServerConfig {
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            self.name.clone()
        } else if !self.host.trim().is_empty() {
            self.host.clone()
        } else {
            "New Server".to_string()
        }
    }

    pub fn to_app_config(&self, base: &AppConfig) -> AppConfig {
        AppConfig {
            host: self.host.clone(),
            user: self.user.clone(),
            password: self.password.clone(),
            database: self.database.clone(),
            mysql_host: base.mysql_host.clone(),
            mysql_user: base.mysql_user.clone(),
            mysql_password: base.mysql_password.clone(),
            mysql_database: base.mysql_database.clone(),
            servers: base.servers.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_mysql_host")]
    pub mysql_host: String,
    #[serde(default)]
    pub mysql_user: String,
    #[serde(default)]
    pub mysql_password: String,
    #[serde(default)]
    pub mysql_database: String,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
}

fn default_mysql_host() -> String {
    "127.0.0.1".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            user: "".to_string(),
            password: "".to_string(),
            database: "".to_string(),
            mysql_host: default_mysql_host(),
            mysql_user: "root".to_string(),
            mysql_password: "".to_string(),
            mysql_database: "".to_string(),
            servers: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn ensure_servers(&mut self) {
        if self.servers.is_empty() && (!self.host.is_empty() || !self.user.is_empty() || !self.database.is_empty()) {
            self.servers.push(ServerConfig {
                name: if self.host.is_empty() { "Default".to_string() } else { self.host.clone() },
                host: self.host.clone(),
                user: self.user.clone(),
                password: self.password.clone(),
                database: self.database.clone(),
            });
        }
    }
}

fn load_or_initialize() -> Result<AppConfig, ConfigError> {
    let config_path = config_path()?;
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let mut config: AppConfig = toml::from_str(&content)?;
        config.ensure_servers();
        if config.mysql_host.is_empty() {
            config.mysql_host = default_mysql_host();
        }
        if config.mysql_user.is_empty() {
            config.mysql_user = "root".to_string();
        }
        return Ok(config);
    }

    let config = AppConfig::default();
    let toml = toml::to_string_pretty(&config).unwrap();
    fs::write(&config_path, toml)?;
    Ok(config)
}
