use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// Client for the Teams Middle-Tier API (/api/mt/), Chat Service API (/api/chatsvc/),
/// and CSA API (/api/csa/).
///
/// Uses `api.spaces.skype.com` token for middle-tier calls,
/// `ic3.teams.office.com` token for chat service calls,
/// and `chatsvcagg.teams.microsoft.com` token for CSA calls.
#[derive(Clone)]
pub struct GraphClient {
    http: Client,
    /// Bearer token for api.spaces.skype.com (used for /api/mt/ calls)
    spaces_token: String,
    /// Bearer token for ic3.teams.office.com (used for /api/chatsvc/ calls)
    ic3_token: String,
    /// Bearer token for graph.microsoft.com
    graph_token: String,
    /// Bearer token for chatsvcagg.teams.microsoft.com (used for /api/csa/ calls)
    csa_token: String,
    /// 2-letter region code (e.g. "de")
    region: String,
    /// Middle-tier region (e.g. "emea")
    mt_region: String,
    /// Direct chat service URL (e.g. "https://de-prod.asyncgw.teams.microsoft.com")
    chat_service_url: String,
}

impl GraphClient {
    pub fn new(
        spaces_token: String,
        ic3_token: String,
        graph_token: String,
        csa_token: String,
        region: String,
        mt_region: String,
        chat_service_url: String,
    ) -> Self {
        Self {
            http: Client::new(),
            spaces_token,
            ic3_token,
            graph_token,
            csa_token,
            region,
            mt_region,
            chat_service_url,
        }
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn mt_region(&self) -> &str {
        &self.mt_region
    }

    /// GET against the Teams Middle-Tier API
    /// Uses the api.spaces.skype.com bearer token
    /// Path is relative: e.g. "beta/users/ME/conversations"
    pub async fn get_mt<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, AppError> {
        let url = format!(
            "https://teams.microsoft.com/api/mt/{}/{}",
            self.mt_region,
            path.trim_start_matches('/')
        );

        tracing::debug!("MT GET {url}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.spaces_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("x-ms-migration", "True")
            .header("x-ms-test-user", "False")
            .header("behavioroverride", "redirectAs404")
            .header("clientinfo", "os=mac; osVer=10.15.7; proc=x86; lcid=en-us; deviceType=1; country=us; clientName=skypeteams; clientVer=1415/26022704215")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("MT API error {status} for {url}: {body}");
            return Err(AppError::Http(status, body));
        }

        Ok(resp.json::<T>().await?)
    }

    /// POST against the Teams Middle-Tier API
    pub async fn post_mt<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, AppError> {
        let url = format!(
            "https://teams.microsoft.com/api/mt/{}/{}",
            self.mt_region,
            path.trim_start_matches('/')
        );

        tracing::debug!("MT POST {url}");

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.spaces_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("x-ms-migration", "True")
            .header("x-ms-test-user", "False")
            .header("behavioroverride", "redirectAs404")
            .header("clientinfo", "os=mac; osVer=10.15.7; proc=x86; lcid=en-us; deviceType=1; country=us; clientName=skypeteams; clientVer=1415/26022704215")
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            tracing::error!("MT API error {status} for {url}: {body_text}");
            return Err(AppError::Http(status, body_text));
        }

        Ok(resp.json::<T>().await?)
    }

    /// GET against the Teams Chat Service API (chatsvc)
    /// Uses the ic3.teams.office.com bearer token
    pub async fn get_chatsvc<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, AppError> {
        let url = format!(
            "https://teams.microsoft.com/api/chatsvc/{}/{}",
            self.region,
            path.trim_start_matches('/')
        );

        tracing::debug!("ChatSvc GET {url}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.ic3_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("behavioroverride", "redirectAs404")
            .header("x-ms-migration", "True")
            .header("x-ms-test-user", "False")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::FORBIDDEN {
                tracing::debug!("ChatSvc 403 for {url}: {body}");
            } else {
                tracing::error!("ChatSvc API error {status} for {url}: {body}");
            }
            return Err(AppError::Http(status, body));
        }

        Ok(resp.json::<T>().await?)
    }

    /// POST against the Teams Chat Service API
    /// Uses the ic3.teams.office.com bearer token
    pub async fn post_chatsvc<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, AppError> {
        let url = format!(
            "https://teams.microsoft.com/api/chatsvc/{}/{}",
            self.region,
            path.trim_start_matches('/')
        );

        tracing::debug!("ChatSvc POST {url}");

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.ic3_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("behavioroverride", "redirectAs404")
            .header("x-ms-migration", "True")
            .header("x-ms-test-user", "False")
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            tracing::error!("ChatSvc API error {status} for {url}: {body_text}");
            return Err(AppError::Http(status, body_text));
        }

        Ok(resp.json::<T>().await?)
    }

    /// GET against Graph API (for /me profile etc.)
    pub async fn get_graph<T: DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, AppError> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.graph_token)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("Graph API error {status} for {url}: {body}");
            return Err(AppError::Http(status, body));
        }

        Ok(resp.json::<T>().await?)
    }

    /// GET against an absolute Chat Service URL (e.g. backwardLink for pagination)
    /// Uses the ic3.teams.office.com bearer token
    pub async fn get_chatsvc_absolute<T: DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, AppError> {
        tracing::debug!("ChatSvc GET (absolute) {url}");

        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.ic3_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("behavioroverride", "redirectAs404")
            .header("x-ms-migration", "True")
            .header("x-ms-test-user", "False")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("ChatSvc API error {status} for {url}: {body}");
            return Err(AppError::Http(status, body));
        }

        Ok(resp.json::<T>().await?)
    }

    /// GET against the CSA (Chat Service Aggregator) API
    /// Uses the chatsvcagg.teams.microsoft.com bearer token
    /// Path is relative to /api/csa/{mt_region}/
    pub async fn get_csa<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, AppError> {
        let url = format!(
            "https://teams.microsoft.com/api/csa/{}/{}",
            self.mt_region,
            path.trim_start_matches('/')
        );

        tracing::debug!("CSA GET {url}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.csa_token)
            .header("x-ms-client-type", "cdlworker")
            .header("x-ms-client-version", "1415/26022704215")
            .header("x-ms-region", &self.region)
            .header("x-ms-migration", "True")
            .header("cache-control", "no-store, no-cache")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("CSA API error {status} for {url}: {body}");
            return Err(AppError::Http(status, body));
        }

        // Read body as text first, then parse — gives precise error messages
        let body = resp.text().await.map_err(reqwest::Error::from)?;
        match serde_json::from_str::<T>(&body) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                // Log the exact position and context around the error
                let line = e.line();
                let col = e.column();
                // For single-line JSON, show chars around the error position
                let start = col.saturating_sub(80);
                let end = (col + 80).min(body.len());
                let context = if body.len() > 200 {
                    &body[start..end.min(body.len())]
                } else {
                    &body
                };
                tracing::error!(
                    "CSA JSON parse error at line {line} col {col}: {e}\n  context: ...{context}..."
                );
                Err(AppError::Json(e))
            }
        }
    }
}
