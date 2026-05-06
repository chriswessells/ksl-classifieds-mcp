mod categories;
mod client;
mod config;
mod db;
mod error;
mod parser;
mod report;
mod tools;
mod types;

use rmcp::{ServiceExt, transport::io::stdio};
use tracing::{info, warn};

use crate::{
    client::{cars::CarsClient, classifieds::ClassifiedsClient},
    config::Config,
    db::Db,
    tools::search::KslMcpServer,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let config = Config::load();
    info!(
        data_dir = %config.data_dir.display(),
        connect_timeout = config.connect_timeout_secs,
        request_timeout = config.request_timeout_secs,
        daily_cap = config.daily_request_cap,
        "ksl-classifieds-mcp starting"
    );

    let db = Db::init(&config.data_dir.join("ksl.db"));
    if db.is_none() {
        warn!("Running in degraded mode — tracking tools unavailable");
    }

    let classifieds_client = ClassifiedsClient::new(&config);
    let cars_client = CarsClient::new(&config);
    let report_server = report::ReportServer::new();
    let server = KslMcpServer::new(classifieds_client, cars_client, db, report_server);

    let (stdin, stdout) = stdio();
    server.serve((stdin, stdout)).await?.waiting().await?;

    Ok(())
}
