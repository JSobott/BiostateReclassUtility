// Purpose: HTTPS client for Intuit QuickBooks Online REST API and OAuth2 token management.
// Owner: Antigravity Agent
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
// tokio_retry removed — manual retry loop used for non-retryable error discrimination

const QBO_API_URL: &str = "https://sandbox-quickbooks.api.intuit.com/v3/company";
const QBO_TOKEN_URL: &str = "https://oauth.platform.intuit.com/oauth2/v1/tokens/bearer";

#[derive(Deserialize, Debug)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub x_refresh_token_expires_in: u64,
    pub token_type: String,
}

pub struct QboClient {
    client: Client,
    realm_id: String,
    access_token: String,
}

impl QboClient {
    pub fn new(realm_id: String, access_token: String) -> Self {
        Self {
            client: Client::new(),
            realm_id,
            access_token,
        }
    }

    pub fn update_access_token(&mut self, access_token: String) {
        self.access_token = access_token;
    }

    pub async fn refresh_oauth_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, Box<dyn std::error::Error>> {
        let client = Client::new();
        let payload = format!(
            "grant_type=refresh_token&refresh_token={}",
            urlencoding::encode(refresh_token)
        );

        let res = client
            .post(QBO_TOKEN_URL)
            .basic_auth(client_id, Some(client_secret))
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let err = res.text().await?;
            return Err(format!("OAuth Refresh failed: {}", err).into());
        }

        let tokens: OAuthTokenResponse = res.json().await?;
        Ok(tokens)
    }

    pub async fn exchange_oauth_token(
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokenResponse, Box<dyn std::error::Error>> {
        let client = Client::new();
        let payload = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}",
            urlencoding::encode(code),
            urlencoding::encode(redirect_uri)
        );

        let res = client
            .post(QBO_TOKEN_URL)
            .basic_auth(client_id, Some(client_secret))
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let err = res.text().await?;
            return Err(format!("OAuth Exchange failed: {}", err).into());
        }

        let tokens: OAuthTokenResponse = res.json().await?;
        Ok(tokens)
    }

    /// Fetches the QBO General Ledger report as a flat list (no account grouping).
    /// Columns returned: Date, Transaction Type, Doc Num, Account Name, Class, Amount.
    pub async fn fetch_general_ledger(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/{}/reports/GeneralLedger?start_date={}&end_date={}&accounting_method=Accrual&columns=tx_date,txn_type,doc_num,acc_name,acc_type,class_name,amt&minorversion=70",
            QBO_API_URL, self.realm_id, start_date, end_date
        );

        tracing::debug!("GL Report URL: {}", url);

        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !res.status().is_success() {
            if res.status().as_u16() == 401 {
                return Err("AuthenticationFailed".into());
            }
            let err_body = res.text().await?;
            return Err(format!("QBO API Report Error: {}", err_body).into());
        }

        let json: Value = res.json().await?;
        let json_str = serde_json::to_string(&json).unwrap_or_default();
        tracing::info!("GL Report JSON received: {} bytes", json_str.len());
        Ok(json)
    }

    /// Fetches all active QBO Classes. Returns a Vec of (Id, FullyQualifiedName) tuples.
    pub async fn fetch_active_classes(
        &self,
    ) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/{}/query?query={}&minorversion=70",
            QBO_API_URL,
            self.realm_id,
            urlencoding::encode("SELECT * FROM Class WHERE Active=true")
        );

        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !res.status().is_success() {
            if res.status().as_u16() == 401 {
                return Err("AuthenticationFailed".into());
            }
            let err_body = res.text().await?;
            return Err(format!("QBO Class Query Error: {}", err_body).into());
        }

        let json: Value = res.json().await?;
        let classes = json
            .get("QueryResponse")
            .and_then(|qr| qr.get("Class"))
            .and_then(|c| c.as_array());

        let mut result = Vec::new();
        if let Some(class_arr) = classes {
            for class in class_arr {
                let id = class
                    .get("Id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = class
                    .get("FullyQualifiedName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !id.is_empty() && !name.is_empty() {
                    result.push((id, name));
                }
            }
        }

        tracing::info!("Fetched {} active QBO classes", result.len());
        Ok(result)
    }

    pub async fn fetch_transaction_by_id(
        &self,
        tx_type: &str,
        tx_id: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = tx_type.to_lowercase();
        let url = format!(
            "{}/{}/{}/{}?minorversion=70",
            QBO_API_URL, self.realm_id, endpoint, tx_id
        );

        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !res.status().is_success() {
            if res.status().as_u16() == 401 {
                return Err("AuthenticationFailed".into());
            }
            let err_body = res.text().await?;
            return Err(
                format!("QBO API Fetch Error ({} {}): {}", tx_type, tx_id, err_body).into(),
            );
        }

        let json: Value = res.json().await?;

        // QBO returns the entity under a PascalCase key that differs from the lowercase
        // endpoint name. We must map explicitly because e.g. json.get("bill") won't find "Bill".
        let entity_key = match tx_type {
            "bill" => "Bill",
            "purchase" => "Purchase",
            "invoice" => "Invoice",
            "salesreceipt" => "SalesReceipt",
            "journalentry" => "JournalEntry",
            "deposit" => "Deposit",
            "vendorcredit" => "VendorCredit",
            other => other,
        };

        let entity = json
            .get(entity_key)
            .or_else(|| {
                json.get("QueryResponse")
                    .and_then(|qr| qr.get(entity_key).and_then(|arr| arr.get(0)))
            })
            .cloned()
            .unwrap_or(json);

        Ok(entity)
    }

    pub async fn update_transaction_class(
        &self,
        tx_type: &str,
        sparse_update: &Value,
        request_id: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        // Critical: TxType determines the endpoint (e.g., /purchase, /journalentry)
        let endpoint = tx_type.to_lowercase();
        let url = format!(
            "{}/{}/{}?minorversion=70",
            QBO_API_URL, self.realm_id, endpoint
        );

        let payload = serde_json::to_string(sparse_update)?;
        tracing::info!(
            "Executing QBO Sparse Update (ReqID: {}): {} bytes",
            request_id,
            payload.len()
        );

        // Manual retry loop — only retries transient errors (429, 5xx).
        // Fatal errors (400, 401, 403, 404) fail immediately without wasting retries.
        let max_retries = 3;
        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            if attempt > 0 {
                let delay_ms = 500u64 * (1u64 << (attempt - 1)); // exponential: 500, 1000, 2000
                let jittered = delay_ms + (rand::random::<u64>() % (delay_ms / 2 + 1));
                tokio::time::sleep(std::time::Duration::from_millis(jittered)).await;
                tracing::info!("Retry attempt {}/{} for QBO update (ReqID: {})", attempt, max_retries, request_id);
            }

            let res = match self
                .client
                .post(&url)
                .header("Request-Id", request_id)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .body(payload.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = format!("Request failed: {}", e);
                    tracing::warn!("QBO request failed (attempt {}): {}", attempt + 1, e);
                    continue; // Network errors are retryable
                }
            };

            if res.status().is_success() {
                let json: Value = res.json().await.map_err(|e| format!("JSON parsing failed: {}", e))?;
                return Ok(json);
            }

            let status = res.status().as_u16();
            let err_body = res.text().await.unwrap_or_else(|_| "Unknown body".to_string());

            // Only retry on transient errors (429 Throttle, 5xx Server Error)
            if status == 429 || status >= 500 {
                tracing::warn!("QBO Transient Error ({}): {}. Will retry...", status, err_body);
                last_error = format!("Transient QBO Error ({}): {}", status, err_body);
                continue;
            }

            // Fatal errors (400, 401, 403, 404, etc.) — fail immediately, no retry
            return Err(format!("Fatal QBO Error ({}) - {}", status, err_body).into());
        }

        Err(last_error.into())
    }

    /// Intuit Security / Diagnosis: Fetch raw CompanyInfo metadata for validation.
    pub async fn fetch_company_info(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/{}/companyinfo/{}?minorversion=70",
            QBO_API_URL, self.realm_id, self.realm_id
        );

        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !res.status().is_success() {
            if res.status().as_u16() == 401 {
                return Err("AuthenticationFailed".into());
            }
            let err_body = res.text().await?;
            return Err(format!("QBO CompanyInfo Fetch Error: {}", err_body).into());
        }

        let json: Value = res.json().await?;
        Ok(json)
    }
}
