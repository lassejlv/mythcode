mod acp_client;
mod cli;
mod input;
mod process;
mod render;
mod session;
mod types;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = cli::run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
