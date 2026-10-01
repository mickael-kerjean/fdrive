use std::io::{BufRead, Write};

use fdrive_core::sdk::Sdk;
use super::{normalize_server, Credentials};

pub async fn login_tty(prefill: &Credentials) -> Option<Credentials> {
    let color = std::env::var_os("NO_COLOR").is_none() && std::env::var("TERM").map_or(true, |t| t != "dumb");
    let paint = |code: &str, text: &str| match color {
        true => format!("\x1b[{code}m{text}\x1b[0m"),
        false => text.to_owned(),
    };
    let _ = say(&format!(
        "\n {} {}\n\n",
        paint("1", "fdrive"),
        paint("2", concat!("v", env!("CARGO_PKG_VERSION"), " · sign in to Filestash")),
    ));

    let mut url = prefill.url.clone();
    let version = loop {
        if url.is_empty() {
            url = match prompt(&paint("1", " Server: ")) {
                Ok(raw) if !raw.is_empty() => normalize_server(&raw),
                _ => return None,
            };
        }
        match Sdk::builder(&url).insecure(prefill.insecure).probe().await {
            Ok(version) => break version,
            Err(err) => {
                let _ = say(&format!(" {} {url} is not a Filestash server: {err}\n\n", paint("31", "✗")));
                url.clear();
            }
        }
    };
    let _ = say(&format!(
        " {} Filestash {version} at {url}\n\n Open this link in a browser, sign in and copy the token shown:\n\n   {}\n\n",
        paint("32", "✓"),
        paint("36", &fdrive_core::sdk::login_url(&url)),
    ));

    let token = loop {
        let token = match prompt(&paint("1", " Token: ")) {
            Ok(token) if !token.is_empty() => token,
            _ => return None,
        };
        let sdk = Sdk::builder(&url).insecure(prefill.insecure).token(token.clone()).ok()?;
        match sdk.ls("/").await {
            Ok(_) => break token,
            Err(err) => {
                let _ = say(&format!(" {} token rejected: {err}\n\n", paint("31", "✗")));
            }
        }
    };
    let _ = say(&format!(" {} Signed in\n\n", paint("32", "✓")));
    Some(Credentials {
        url,
        token,
        insecure: prefill.insecure,
    })
}

fn say(message: &str) -> std::io::Result<()> {
    let mut tty = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
    tty.write_all(message.as_bytes())?;
    tty.flush()
}

fn prompt(label: &str) -> std::io::Result<String> {
    say(label)?;
    let mut line = String::new();
    if std::io::BufReader::new(std::fs::File::open("/dev/tty")?).read_line(&mut line)? == 0 {
        say("\n")?;
    }
    Ok(line.trim().to_owned())
}
