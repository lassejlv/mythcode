mod acp_client;
mod cli;
mod extensions;
mod input;
mod process;
mod session;
mod spinner;
mod terminal_ui;
mod tui;
mod types;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = cli::run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
