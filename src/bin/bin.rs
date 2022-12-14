use std::process::ExitCode;

use librhodos::serve;
use librhodos::settings::{self, dotenv_override};
use librhodos::startup;
use librhodos::telemetry::{get_subscriber, init_subscriber};

#[tokio::main]
async fn main() -> ExitCode {
    let subscriber = get_subscriber(
        "RUST_LOG".to_string(),
        "rhodos=debug,tower_http=debug".into(),
        std::io::stdout,
    );
    init_subscriber(subscriber);

    let mut global_config = settings::Settings::new(None, None)
        .map_err(|e| {
            eprintln!("Failed to get settings: {}", e);
            ExitCode::FAILURE
        })
        .unwrap();

    dotenv_override(&mut global_config);

    // Process command line arguments
    let args: startup::Args = startup::process_command_line();
    if args.flag_init_db.is_some() {
        if let Err(e) = startup::initialize_database(&args, &global_config).await {
            eprintln!("failed to initialize database: {}", e);
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    tracing::info!("Application Started");

    let (router, listener) = startup::build(&global_config, None).await;
    tokio::join!(serve(router, listener));

    tracing::info!("Application Stopped");
    ExitCode::SUCCESS
}
