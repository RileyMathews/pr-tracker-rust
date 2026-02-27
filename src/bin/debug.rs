#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting local development entrypoint...");

    notify_rust::Notification::new()
        .summary("Test")
        .body("This is the notification body")
        .appname("pr-tracker")
        .show()?;

    Ok(())
}
