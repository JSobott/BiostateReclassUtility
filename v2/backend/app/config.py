# Constants for the BiostateReclassUtility v2
# Ported from Rust src/qbo_client.rs, src/server.rs, src/llm_engine.rs

QBO_API_BASE = "https://sandbox-quickbooks.api.intuit.com/v3/company"
QBO_TOKEN_URL = "https://oauth.platform.intuit.com/oauth2/v1/tokens/bearer"
QBO_MINOR_VERSION = "70"
OAUTH_REDIRECT_URI = "https://developer.intuit.com/v2/OAuth2Playground/RedirectUrl"
OAUTH_SCOPE = "com.intuit.quickbooks.accounting"
OAUTH_AUTH_URL = "https://appcenter.intuit.com/connect/oauth2"

KEYCHAIN_SERVICE = "ReclassUtility"
KEYCHAIN_ACCOUNTS = {
    "access_token": "qbo_access_token",
    "refresh_token": "qbo_refresh_token",
    "realm_id": "qbo_realm_id",
    "client_id": "qbo_client_id",
    "client_secret": "qbo_client_secret",
    "anthropic_api_key": "anthropic_api_key",
}

ANTHROPIC_MODEL = "claude-opus-4-6"
SERVER_PORT = 3030

# Concurrency limits (matching Rust version)
QBO_CONCURRENT_FETCHES = 3
LLM_CONCURRENT_REQUESTS = 5
QBO_FETCH_DELAY_MS = 500
GL_CHUNK_DAYS = 29

# Retry config
MAX_RETRIES = 3
RETRY_BASE_DELAY_MS = 500

ALLOWED_RULE_FIELDS = ["account_type", "account", "tx_type", "entity_name"]

# Report type -> API entity mapping (from sync.rs map_report_type_to_api_entity)
REPORT_TYPE_MAP = {
    "Expense": "purchase",
    "Check": "purchase",
    "Cash Expense": "purchase",
    "Cash Purchase": "purchase",
    "Credit Card Expense": "purchase",
    "Credit Card Charge": "purchase",
    "Bill": "bill",
    "Invoice": "invoice",
    "Sales Receipt": "salesreceipt",
    "Journal Entry": "journalentry",
    "Deposit": "deposit",
    "Vendor Credit": "vendorcredit",
    "Credit Card Credit": "vendorcredit",
}

# API entity type -> QBO response key mapping (from qbo_client.rs fetch_transaction_by_id)
ENTITY_KEY_MAP = {
    "bill": "Bill",
    "purchase": "Purchase",
    "invoice": "Invoice",
    "salesreceipt": "SalesReceipt",
    "journalentry": "JournalEntry",
    "deposit": "Deposit",
    "vendorcredit": "VendorCredit",
}

# Detail types that support ClassRef
CLASSIFIABLE_DETAIL_TYPES = [
    "AccountBasedExpenseLineDetail",
    "ItemBasedExpenseLineDetail",
    "JournalEntryLineDetail",
    "SalesItemLineDetail",
]
