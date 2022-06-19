#![feature(hash_raw_entry)]

use color_eyre::Result;

pub mod archive;
pub mod client;
pub mod command;
pub mod opts;
pub mod scrape;
pub mod filesystem;
pub mod search;
pub mod utils;
pub mod pick;

fn install_tracing() -> color_eyre::Result<()> {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stderr());
    std::mem::forget(guard);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_span_events(FmtSpan::CLOSE);
    // .pretty();
    let filter_layer =
        tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("kscrpr=error".parse()?)
        ;

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    install_tracing()?;

    color_eyre::install()?;

    command::do_stuff().await?;

    Ok(())
}
