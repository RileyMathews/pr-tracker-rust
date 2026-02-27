#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        pr_tracker_rust::tui_app::run().await
    } else {
        pr_tracker_rust::cli_app::run_from_args(args).await
    }
}
