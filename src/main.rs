// Purpose: CLI entrypoint and Axum server initialization for ReclassUtility.
// Owner: Antigravity Agent
pub mod db;
pub mod llm_engine;
pub mod models;
pub mod qbo_client;
pub mod secrets;
pub mod server;
pub mod sync;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Skip the web server and run a one-off sync to pull unclassified transactions from QBO
    #[arg(short, long)]
    sync: bool,

    /// Start date for the sync (YYYY-MM-DD format). Defaults to last 30 days if omitted.
    #[arg(long)]
    start_date: Option<String>,

    /// End date for the sync (YYYY-MM-DD format). Defaults to today if omitted.
    #[arg(long)]
    end_date: Option<String>,

    /// Override the QBO Client ID stored in the Keychain
    #[arg(long)]
    client_id: Option<String>,

    /// Override the QBO Client Secret stored in the Keychain
    #[arg(long)]
    client_secret: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting ReclassUtility...");

    // Parse CLI arguments
    let args = Args::parse();

    if let (Some(cid), Some(csec)) = (&args.client_id, &args.client_secret) {
        secrets::store_client_credentials(cid, csec)?;
        tracing::info!("Successfully updated QBO Client Credentials in Keychain.");
    }

    // Initialize DB
    let db = db::init_db()?;
    tracing::info!("Database initialized successfully.");

    if args.sync {
        tracing::info!("Running in SYNC mode. Fetching live QBO data...");
        sync::run_sync_job(db, args.start_date, args.end_date).await?;
        tracing::info!("Sync complete. Run without --sync to start the review server.");
    } else {
        // Start Server
        tracing::info!("Starting Axum Web Server...");
        server::start_server(db).await?;
    }

    Ok(())
}
