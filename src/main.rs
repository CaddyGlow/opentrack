use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use opentrack::cache;
use opentrack::cache::store;
use opentrack::cli;
use opentrack::cli::{
    AddArgs, CacheArgs, Commands, ConfigArgs, ListArgs, RemoveArgs, TrackArgs, WatchArgs,
};
use opentrack::config;
use opentrack::config::{Config, OutputMode, ParcelEntry};
use opentrack::notifications;
use opentrack::providers::{Provider, ProviderRegistry};
use opentrack::tracking::TrackOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    let cli = cli::Cli::parse();
    match cli.command {
        Commands::Config(args) => run_config(args).await,
        Commands::Tui => {
            opentrack::tui::run().await?;
            Ok(())
        }
        command => {
            let mut config = config::load().await?;

            match command {
                Commands::Track(args) => {
                    let http_client = opentrack::providers::build_http_client(&config)?;
                    let registry = ProviderRegistry::new(http_client, &config);
                    let result = run_track(&registry, &config, args).await;
                    registry.shutdown().await;
                    result
                }
                Commands::Add(args) => {
                    let http_client = opentrack::providers::build_http_client(&config)?;
                    let registry = ProviderRegistry::new(http_client, &config);
                    let result = run_add(&registry, &mut config, args).await;
                    registry.shutdown().await;
                    result
                }
                Commands::List(args) => run_list(&config, args).await,
                Commands::Remove(args) => run_remove(&mut config, args).await,
                Commands::Watch(args) => {
                    let http_client = opentrack::providers::build_http_client(&config)?;
                    let registry = ProviderRegistry::new(http_client.clone(), &config);
                    let notifiers =
                        notifications::build_notifiers(&config.notifications, &http_client);
                    let result = run_watch(&registry, &config, &notifiers, args).await;
                    registry.shutdown().await;
                    result
                }
                Commands::Cache(args) => run_cache(args).await,
                Commands::Config(_) | Commands::Tui => Ok(()),
            }
        }
    }
}

fn init_tracing() -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

async fn run_track(
    registry: &ProviderRegistry,
    config: &Config,
    args: TrackArgs,
) -> anyhow::Result<()> {
    let provider = resolve_provider(registry, args.provider.as_deref(), &args.id)?;
    let options = TrackOptions {
        postcode: args.postcode,
        lang: args.lang,
        no_cache: args.no_cache,
    };

    let info = track_with_cache(provider, config, &args.id, &options).await?;
    let as_json = args.json || matches!(config.general.output, OutputMode::Json);
    cli::output::print_tracking(&info, as_json)?;
    Ok(())
}

async fn run_add(
    registry: &ProviderRegistry,
    config: &mut Config,
    args: AddArgs,
) -> anyhow::Result<()> {
    registry.get_by_id(&args.provider)?;

    let exists = config
        .parcels
        .iter()
        .any(|parcel| parcel.id == args.id && parcel.provider == args.provider);
    if exists {
        anyhow::bail!("parcel already exists: {}/{}", args.provider, args.id);
    }

    config.parcels.push(ParcelEntry {
        id: args.id,
        provider: args.provider,
        label: args.label,
        postcode: args.postcode,
        lang: args.lang,
        notify: args.notify,
    });
    config::save(config).await?;

    println!("parcel added");
    Ok(())
}

async fn run_list(config: &Config, args: ListArgs) -> anyhow::Result<()> {
    let mut rows = Vec::with_capacity(config.parcels.len());

    for parcel in &config.parcels {
        let cached = store::read(&parcel.provider, &parcel.id).await?;
        let (status, last_event_at, cached_at) = if let Some(entry) = cached {
            (
                Some(entry.info.status.to_string()),
                entry.info.events.first().map(|event| event.timestamp),
                Some(entry.cached_at),
            )
        } else {
            (None, None, None)
        };

        rows.push(cli::output::ListRow {
            id: parcel.id.clone(),
            provider: parcel.provider.clone(),
            label: parcel.label.clone(),
            status,
            last_event_at,
            cached_at,
        });
    }

    let as_json = args.json || matches!(config.general.output, OutputMode::Json);
    cli::output::print_list(&rows, as_json)?;
    Ok(())
}

async fn run_remove(config: &mut Config, args: RemoveArgs) -> anyhow::Result<()> {
    let before = config.parcels.len();
    config.parcels.retain(|parcel| parcel.id != args.id);
    let removed = before.saturating_sub(config.parcels.len());
    config::save(config).await?;

    println!("removed {removed} parcel(s)");
    Ok(())
}

async fn run_watch(
    registry: &ProviderRegistry,
    config: &Config,
    notifiers: &[(
        Vec<notifications::NotificationTrigger>,
        Box<dyn notifications::Notifier>,
    )],
    args: WatchArgs,
) -> anyhow::Result<()> {
    if config.parcels.is_empty() {
        println!("no parcels configured");
        return Ok(());
    }

    let interval = args
        .interval
        .unwrap_or(config.general.watch_interval)
        .max(1);

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        for parcel in &config.parcels {
            let provider = match registry.get_by_id(&parcel.provider) {
                Ok(provider) => provider,
                Err(err) => {
                    tracing::warn!(provider = %parcel.provider, error = %err, "skipping parcel due to provider error");
                    continue;
                }
            };

            let options = TrackOptions {
                postcode: parcel.postcode.clone(),
                lang: parcel.lang.clone(),
                no_cache: true,
            };

            let old = store::read(provider.id(), &parcel.id)
                .await?
                .map(|entry| entry.info);

            let info = match provider.track(&parcel.id, &options).await {
                Ok(info) => info,
                Err(err) => {
                    tracing::warn!(provider = %parcel.provider, parcel_id = %parcel.id, error = %err, "watch poll failed");
                    continue;
                }
            };

            let send_notifications = parcel.notify.unwrap_or(true);
            if send_notifications {
                let triggers = notifications::evaluate_triggers(old.as_ref(), &info);
                for trigger in triggers {
                    let event = notifications::build_event(
                        trigger,
                        parcel.label.clone(),
                        old.as_ref(),
                        &info,
                    );
                    notifications::dispatch(notifiers, &event).await;
                }
            }

            store::write(provider.id(), &parcel.id, &info).await?;

            if !args.quiet {
                let line = cli::output::format_watch_line(
                    Utc::now(),
                    provider.id(),
                    &parcel.id,
                    parcel.label.as_deref(),
                    &info.status,
                    info.events.first().map(|event| event.description.as_str()),
                );
                println!("{line}");
            }
        }

        if args.once {
            break;
        }

        #[cfg(unix)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received interrupt signal, exiting watch loop");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, exiting watch loop");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval)) => {}
        }

        #[cfg(not(unix))]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received interrupt signal, exiting watch loop");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval)) => {}
        }
    }

    Ok(())
}

async fn run_config(args: ConfigArgs) -> anyhow::Result<()> {
    match args.command {
        cli::ConfigCommand::Path => {
            println!("{}", config::config_path()?.display());
        }
        cli::ConfigCommand::Edit => {
            config::edit().await?;
        }
    }

    Ok(())
}

async fn run_cache(args: CacheArgs) -> anyhow::Result<()> {
    match args.command {
        cli::CacheCommand::Clear(clear) => {
            let removed =
                store::clear(clear.provider.as_deref(), clear.parcel_id.as_deref()).await?;
            println!("deleted {removed} cache entries");
        }
    }

    Ok(())
}

fn resolve_provider<'a>(
    registry: &'a ProviderRegistry,
    requested: Option<&str>,
    parcel_id: &str,
) -> anyhow::Result<&'a dyn Provider> {
    if let Some(provider_id) = requested {
        return registry
            .get_by_id(provider_id)
            .with_context(|| format!("unknown provider '{provider_id}'"));
    }

    registry
        .auto_detect(parcel_id)
        .with_context(|| format!("could not auto-detect provider for '{parcel_id}'"))
}

async fn track_with_cache(
    provider: &dyn Provider,
    config: &Config,
    parcel_id: &str,
    options: &TrackOptions,
) -> anyhow::Result<opentrack::tracking::TrackingInfo> {
    if !options.no_cache {
        if let Some(cached) =
            cache::read_fresh(provider.id(), parcel_id, config.general.cache_ttl).await?
        {
            return Ok(cached);
        }
    }

    let info = provider.track(parcel_id, options).await?;
    store::write(provider.id(), parcel_id, &info).await?;
    Ok(info)
}
