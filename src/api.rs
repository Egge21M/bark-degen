use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Method};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::protocol::{Bet, BetRequest, Config, Draft};

pub struct Api {
    client: Client,
    base: Url,
}

pub fn endpoint(value: &str) -> Result<Url> {
    let mut url = Url::parse(value).context("invalid service URL")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "service URL must use HTTPS (HTTP is allowed only on loopback for testing)"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "service URL must not contain credentials, query, or fragment"
    );
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

impl Api {
    pub fn new(base: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .user_agent(concat!("bark-degen/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base: endpoint(base)?,
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&BetRequest>,
    ) -> Result<T> {
        let mut request = self
            .client
            .request(method, self.base.join(path)?)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        // Never retry POSTs: the service does not publish an idempotency contract.
        let mut response = request
            .send()
            .await
            .map_err(|e| e.without_url())
            .context("Barkdice request failed")?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
            ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "Barkdice response exceeds 2 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            // Do not echo arbitrary response bodies, which can include bearer tokens.
            anyhow::bail!("Barkdice returned HTTP {status}");
        }
        serde_json::from_slice(&bytes).context("invalid Barkdice JSON response")
    }

    pub async fn config(&self) -> Result<Config> {
        self.request(Method::GET, "api/config", None).await
    }
    pub async fn commit(&self) -> Result<Draft> {
        self.request(Method::POST, "api/commit", None).await
    }
    pub async fn quote(&self, body: &BetRequest) -> Result<String> {
        #[derive(Deserialize)]
        struct Created {
            token: String,
        }
        let created: Created = self.request(Method::POST, "api/bets", Some(body)).await?;
        valid_token(&created.token)?;
        Ok(created.token)
    }
    pub async fn bet(&self, token: &str) -> Result<Bet> {
        valid_token(token)?;
        self.request(Method::GET, &format!("api/bets/{token}"), None)
            .await
    }
}

fn valid_token(token: &str) -> Result<()> {
    ensure!(
        !token.is_empty()
            && token.len() <= 512
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid bet access token"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[tokio::test]
    async fn reads_config_and_does_not_retry_failed_commit() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            for (expected, status, body) in [
                (
                    "GET /api/config ",
                    "200 OK",
                    include_str!("../tests/fixtures/config.json"),
                ),
                ("POST /api/commit ", "503 Service Unavailable", "{}"),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 8192];
                let n = socket.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..n]).starts_with(expected));
                write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let api = Api::new(&format!("http://{address}")).unwrap();
        assert_eq!(api.config().await.unwrap().network, "mainnet");
        assert!(api.commit().await.is_err());
        thread.join().unwrap();
    }

    #[test]
    fn rejects_remote_plaintext_and_token_paths() {
        assert!(Api::new("http://barkdice.com").is_err());
        assert!(Api::new("https://user:password@barkdice.com").is_err());
        assert!(valid_token("../config?token=secret").is_err());
        assert!(valid_token("abc-123_xyz").is_ok());
    }
}
