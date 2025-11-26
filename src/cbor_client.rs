use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// CBOR HTTP client for fast binary API communication
/// Uses /api/v2/cbor/* endpoints instead of /api/v1/* JSON endpoints
pub struct CborClient {
    client: Client,
    base_url: String,
    token: String,
}

impl CborClient {
    pub fn new(api_url: &str, token: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: api_url.to_string(),
            token: token.to_string(),
        }
    }

    /// GET request with CBOR response
    pub async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}/api/v2/cbor{}", self.base_url, path);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/cbor")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.bytes().await?;
            return Err(anyhow::anyhow!("HTTP {status}: {body:?}"));
        }

        let body = response.bytes().await?;
        let decoded: T = ciborium::from_reader(&body[..])?;
        Ok(decoded)
    }

    /// POST request with CBOR request and response
    #[allow(clippy::future_not_send)] // HTTP client operations don't need Send constraint
    pub async fn post<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        data: &T,
    ) -> Result<R> {
        let url = format!("{}/api/v2/cbor{}", self.base_url, path);

        // Encode request as CBOR
        let mut cbor_body = Vec::new();
        ciborium::into_writer(data, &mut cbor_body)?;

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/cbor")
            .header("Accept", "application/cbor")
            .body(cbor_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.bytes().await?;
            return Err(anyhow::anyhow!("HTTP {status}: {body:?}"));
        }

        let body = response.bytes().await?;
        let decoded: R = ciborium::from_reader(&body[..])?;
        Ok(decoded)
    }

    /// PUT request with binary body (for NAR uploads)
    pub async fn put_binary(&self, path: &str, body: Vec<u8>) -> Result<()> {
        let url = format!("{}/api/v2/cbor{}", self.base_url, path);

        let response = self
            .client
            .post(&url) // CBOR endpoints use POST for uploads
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/x-nix-archive")
            .header("X-Async", "true")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.bytes().await?;
            return Err(anyhow::anyhow!("HTTP {status}: {body:?}"));
        }

        Ok(())
    }

    /// Upload binary data in chunks (4MB per chunk for large files)
    #[allow(dead_code)]
    pub async fn put_binary_chunked(
        &self,
        path: &str,
        data: Vec<u8>,
        chunk_size: usize,
    ) -> Result<()> {
        const DEFAULT_CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB
        let chunk_size = if chunk_size == 0 {
            DEFAULT_CHUNK_SIZE
        } else {
            chunk_size
        };

        // If file is smaller than chunk size, upload as single request
        if data.len() <= chunk_size {
            return self.put_binary(path, data).await;
        }

        // For large files, upload in chunks
        let total_chunks = data.len().div_ceil(chunk_size);
        for (chunk_idx, chunk) in data.chunks(chunk_size).enumerate() {
            let url = format!("{}/api/v2/cbor{}", self.base_url, path);
            let chunk_header = format!(
                "bytes {}-{}/{}",
                chunk_idx * chunk_size,
                (chunk_idx + 1) * chunk_size.min(data.len()) - 1,
                data.len()
            );

            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Content-Type", "application/x-nix-archive")
                .header("Content-Range", chunk_header)
                .header("X-Async", "true")
                .body(chunk.to_vec())
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.bytes().await?;
                return Err(anyhow::anyhow!(
                    "HTTP {} on chunk {}/{}: {:?}",
                    status,
                    chunk_idx + 1,
                    total_chunks,
                    body
                ));
            }
        }

        Ok(())
    }

    /// POST request with CBOR request body (for uploads)
    #[allow(clippy::future_not_send)] // HTTP client operations don't need Send constraint
    pub async fn put_cbor<T: Serialize>(&self, path: &str, data: &T) -> Result<()> {
        let url = format!("{}/api/v2/cbor{}", self.base_url, path);

        let mut cbor_body = Vec::new();
        ciborium::into_writer(data, &mut cbor_body)?;

        let response = self
            .client
            .post(&url) // CBOR endpoints use POST
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/cbor")
            .header("X-Async", "true")
            .body(cbor_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.bytes().await?;
            return Err(anyhow::anyhow!("HTTP {status}: {body:?}"));
        }

        Ok(())
    }
}

/// CBOR request/response types
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheInfo {
    pub name: String,
    pub public: bool,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NarInfoRequest {
    pub narinfo: String,
}
