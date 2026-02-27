#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pr_tracker_rust::cli_app::run_from_args(std::env::args()).await
}
