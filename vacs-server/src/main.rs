use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::watch;
use tracing_subscriber::Layer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vacs_server::auth::layer::setup_auth_layer;
use vacs_server::build::BuildInfo;
use vacs_server::config::AppConfig;
use vacs_server::dataset::DatasetManager;
use vacs_server::metrics::NetworkDatasetMetrics;
use vacs_server::metrics::setup_prometheus_metric_layer;
use vacs_server::ratelimit::RateLimiters;
use vacs_server::release::UpdateChecker;
use vacs_server::release::policy::Policy;
use vacs_server::routes::{create_app, create_metrics_app};
use vacs_server::state::AppState;
use vacs_server::store::Store;
use vacs_server::store::redis::RedisStore;
use vacs_vatsim::coverage::network::Network;
use vacs_vatsim::data_feed::VatsimDataFeed;
use vacs_vatsim::slurper::SlurperClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!(
            "{}=trace,vacs_=trace,tower_http=debug,tower_sessions=debug,axum::rejection=trace",
            env!("CARGO_CRATE_NAME")
        )
        .into()
    });

    let fmt_layer = if std::env::var("RUST_LOG_JSON").is_ok() {
        tracing_subscriber::fmt::layer().json().boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    let build_info = BuildInfo::gather();
    tracing::info!(?build_info);

    let config = AppConfig::parse()?;

    let policy = Policy::new(&config.updates.policy_path)?;
    let updates = UpdateChecker::new(config.updates.catalog.to_catalog().await?, policy);

    let redis_store = RedisStore::new(&config.redis).await?;
    let redis_pool = redis_store.get_pool().clone();

    let slurper = SlurperClient::new(config.vatsim.slurper_base_url.as_str())?;
    let data_feed = Arc::new(VatsimDataFeed::new(
        config.vatsim.data_feed_url.as_str(),
        config.vatsim.data_feed_timeout,
    )?);

    let rate_limiters = RateLimiters::from(config.rate_limiters);

    let ice_config_provider = config.ice.create_provider()?;

    let (prom_layer, prom_handle) = setup_prometheus_metric_layer();

    let (shutdown_tx, shutdown_rx) = watch::channel(());

    // Set up the dataset manager for fetching updates from GitHub (if configured).
    let dataset_manager = match &config.admin.dataset {
        Some(dataset_config) => {
            Some(DatasetManager::new(dataset_config, &config.vatsim.coverage_dir).await?)
        }
        None => {
            tracing::info!("No dataset repository configured, skipping GitHub sync");
            None
        }
    };

    // Try syncing from GitHub first; fall back to loading from disk.
    let network = match &dataset_manager {
        Some(dm) => match dm.sync_on_startup().await? {
            Some(network) => {
                tracing::info!("Using freshly downloaded dataset from GitHub");
                network
            }
            None => {
                tracing::info!(path = ?config.vatsim.coverage_dir, "Remote dataset matches local copy, loading from disk");
                Network::load_from_dir(&config.vatsim.coverage_dir).map_err(|err| {
                    anyhow::anyhow!("Failed to load network coverage data: {err:?}")
                })?
            }
        },
        None => {
            tracing::info!(path = ?config.vatsim.coverage_dir, "Loading network coverage data from disk");
            Network::load_from_dir(&config.vatsim.coverage_dir)
                .map_err(|err| anyhow::anyhow!("Failed to load network coverage data: {err:?}"))?
        }
    };

    NetworkDatasetMetrics::set_dataset_size(
        network.positions_count(),
        network.stations_count(),
        network.profiles_count(),
    );

    let app_state = Arc::new(AppState::new(
        config.clone(),
        updates,
        Store::Redis(redis_store),
        slurper,
        data_feed,
        network,
        rate_limiters,
        shutdown_rx.clone(),
        ice_config_provider,
        dataset_manager,
    ));

    let auth_layer = setup_auth_layer(&config, redis_pool).await?;

    let app = create_app(
        auth_layer,
        Some(prom_layer),
        config.server.client_ip_source.clone(),
        config.server.debug_endpoints,
    );
    let listener = tokio::net::TcpListener::bind(config.server.bind_addr).await?;
    tracing::info!(bind_addr = ?listener.local_addr(), "Started main listener");

    let metrics_app = create_metrics_app(prom_handle);
    let metrics_listener = tokio::net::TcpListener::bind(config.server.metrics_bind_addr).await?;
    tracing::info!(bind_addr = ?metrics_listener.local_addr(), "Started metrics listener");

    let controller_update_task = AppState::start_controller_update_task(
        app_state.clone(),
        config.vatsim.controller_update_interval,
    );

    let metrics_server = axum::serve(metrics_listener, metrics_app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(shutdown_tx.clone()));

    let server = axum::serve(
        listener,
        app.with_state(app_state)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(shutdown_tx));

    tokio::try_join!(metrics_server, server)?;

    if let Err(err) = controller_update_task.await {
        tracing::warn!(?err, "Controller update task finished with error");
    }

    Ok(())
}

async fn shutdown_signal(shutdown_tx: watch::Sender<()>) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("Shutdown signal received, terminating gracefully...");

    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");
}
