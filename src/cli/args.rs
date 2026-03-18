use clap::{Args, Parser, Subcommand};

const APP_VERSION: &str = env!("OPENTRACK_VERSION");

#[derive(Debug, Parser)]
#[command(name = "opentrack")]
#[command(version = APP_VERSION)]
#[command(about = "Async parcel tracking CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Track(TrackArgs),
    Add(AddArgs),
    List(ListArgs),
    Remove(RemoveArgs),
    Watch(WatchArgs),
    Tui,
    Config(ConfigArgs),
    Cache(CacheArgs),
}

#[derive(Debug, Args)]
pub struct TrackArgs {
    pub id: String,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub postcode: Option<String>,
    #[arg(long)]
    pub lang: Option<String>,
    #[arg(long)]
    pub no_cache: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    pub id: String,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
    #[arg(long)]
    pub postcode: Option<String>,
    #[arg(long)]
    pub lang: Option<String>,
    #[arg(long)]
    pub notify: Option<bool>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    pub id: String,
}

#[derive(Debug, Args)]
pub struct WatchArgs {
    #[arg(long)]
    pub interval: Option<u64>,
    #[arg(long)]
    pub once: bool,
    #[arg(long)]
    pub quiet: bool,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Path,
    Edit,
}

#[derive(Debug, Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommand,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    Clear(CacheClearArgs),
}

#[derive(Debug, Args)]
pub struct CacheClearArgs {
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long = "id")]
    pub parcel_id: Option<String>,
}
