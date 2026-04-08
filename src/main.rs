mod acp_client;
mod cli;
mod input;
mod process;
mod session;
mod spinner;
mod tui;
mod types;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = cli::run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
