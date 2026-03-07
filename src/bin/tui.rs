#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pr_tracker_rust::tui::run().await
}
