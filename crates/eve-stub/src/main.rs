//! `eve-stub --cert <pem> --key <pem> [--listen 127.0.0.1:9443] [--base-url https://host:port] [--client-id ..] [--client-secret ..] [--control-token ..]`

use std::process::ExitCode;

use eve_stub::{Stub, StubConfig};

struct Args {
    cert: String,
    key: String,
    listen: String,
    client_id: String,
    client_secret: String,
    control_token: String,
    base_url: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let (mut cert, mut key) = (None, None);
    let mut listen = "127.0.0.1:9443".to_owned();
    let mut client_id = "stub-client".to_owned();
    let mut client_secret = "stub-secret".to_owned();
    let mut control_token = "stub-control-token".to_owned();
    let mut base_url = None;
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--cert" => cert = Some(value()?),
            "--key" => key = Some(value()?),
            "--listen" => listen = value()?,
            "--client-id" => client_id = value()?,
            "--client-secret" => client_secret = value()?,
            "--control-token" => control_token = value()?,
            "--base-url" => base_url = Some(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        cert: cert.ok_or("--cert is required")?,
        key: key.ok_or("--key is required")?,
        listen,
        client_id,
        client_secret,
        control_token,
        base_url,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let cert = std::fs::read(&args.cert).map_err(|e| format!("cannot read {}: {e}", args.cert))?;
    let key = std::fs::read(&args.key).map_err(|e| format!("cannot read {}: {e}", args.key))?;
    let listener = std::net::TcpListener::bind(&args.listen)?;
    let mut config = StubConfig::new(&args.client_id, &args.client_secret, &args.control_token);
    config.base_url = args.base_url;
    let stub = Stub::start(config, listener, cert, key).await?;
    println!("eve-stub listening on {}", stub.base_url);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
