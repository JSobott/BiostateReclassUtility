"""HTTPS client for Intuit QuickBooks Online REST API and OAuth2 token management.

Ported from Rust src/qbo_client.rs to async Python using httpx.
"""

import asyncio
import logging
import random
from typing import Any
from urllib.parse import quote

import httpx

from .config import (
    ENTITY_KEY_MAP,
    MAX_RETRIES,
    QBO_API_BASE,
    QBO_MINOR_VERSION,
    QBO_TOKEN_URL,
    RETRY_BASE_DELAY_MS,
)

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Custom Exceptions
# ---------------------------------------------------------------------------

class AuthenticationError(Exception):
    """Raised when QBO returns 401 Unauthorized."""


class QboApiError(Exception):
    """Raised for non-auth QBO API failures."""

    def __init__(self, message: str, status_code: int | None = None) -> None:
        super().__init__(message)
        self.status_code = status_code


# ---------------------------------------------------------------------------
# OAuth Token Response
# ---------------------------------------------------------------------------

class OAuthTokenResponse:
    """Mirrors the Intuit OAuth2 token endpoint JSON response."""

    __slots__ = (
        "access_token",
        "refresh_token",
        "expires_in",
        "x_refresh_token_expires_in",
        "token_type",
    )

    def __init__(self, data: dict[str, Any]) -> None:
        self.access_token: str = data["access_token"]
        self.refresh_token: str = data["refresh_token"]
        self.expires_in: int = data["expires_in"]
        self.x_refresh_token_expires_in: int = data["x_refresh_token_expires_in"]
        self.token_type: str = data["token_type"]


# ---------------------------------------------------------------------------
# QBO Client
# ---------------------------------------------------------------------------

class QboClient:
    """Async HTTP client for the QuickBooks Online REST API (sandbox).

    Provides methods for OAuth token management, General Ledger retrieval,
    class queries, transaction fetch/update, and company info.
    """

    def __init__(self, realm_id: str, access_token: str) -> None:
        self._realm_id = realm_id
        self._access_token = access_token
        self._client = httpx.AsyncClient(timeout=httpx.Timeout(30.0))

    # -- helpers --

    @property
    def _base_url(self) -> str:
        return f"{QBO_API_BASE}/{self._realm_id}"

    def _auth_headers(self) -> dict[str, str]:
        return {
            "Authorization": f"Bearer {self._access_token}",
            "Accept": "application/json",
        }

    def update_access_token(self, access_token: str) -> None:
        self._access_token = access_token

    async def close(self) -> None:
        """Gracefully close the underlying httpx client."""
        await self._client.aclose()

    # ------------------------------------------------------------------
    # Static OAuth helpers (no instance required)
    # ------------------------------------------------------------------

    @staticmethod
    async def refresh_oauth_token(
        client_id: str,
        client_secret: str,
        refresh_token: str,
    ) -> OAuthTokenResponse:
        """Exchange a refresh token for a new access/refresh token pair."""
        payload = (
            f"grant_type=refresh_token"
            f"&refresh_token={quote(refresh_token, safe='')}"
        )
        async with httpx.AsyncClient(timeout=httpx.Timeout(30.0)) as client:
            res = await client.post(
                QBO_TOKEN_URL,
                content=payload,
                headers={
                    "Accept": "application/json",
                    "Content-Type": "application/x-www-form-urlencoded",
                },
                auth=(client_id, client_secret),
            )

        if not res.is_success:
            err = res.text
            logger.error("OAuth refresh failed (%d): %s", res.status_code, err)
            raise AuthenticationError(f"OAuth Refresh failed: {err}")

        return OAuthTokenResponse(res.json())

    @staticmethod
    async def exchange_oauth_token(
        client_id: str,
        client_secret: str,
        code: str,
        redirect_uri: str,
    ) -> OAuthTokenResponse:
        """Exchange an authorization code for tokens."""
        payload = (
            f"grant_type=authorization_code"
            f"&code={quote(code, safe='')}"
            f"&redirect_uri={quote(redirect_uri, safe='')}"
        )
        async with httpx.AsyncClient(timeout=httpx.Timeout(30.0)) as client:
            res = await client.post(
                QBO_TOKEN_URL,
                content=payload,
                headers={
                    "Accept": "application/json",
                    "Content-Type": "application/x-www-form-urlencoded",
                },
                auth=(client_id, client_secret),
            )

        if not res.is_success:
            err = res.text
            logger.error("OAuth exchange failed (%d): %s", res.status_code, err)
            raise AuthenticationError(f"OAuth Exchange failed: {err}")

        return OAuthTokenResponse(res.json())

    # ------------------------------------------------------------------
    # Instance API methods
    # ------------------------------------------------------------------

    async def fetch_general_ledger(
        self,
        start_date: str,
        end_date: str,
    ) -> dict[str, Any]:
        """Fetch the QBO General Ledger report as raw JSON.

        Columns: tx_date, txn_type, doc_num, acc_name, acc_type, class_name, amt.
        """
        url = (
            f"{self._base_url}/reports/GeneralLedger"
            f"?start_date={start_date}"
            f"&end_date={end_date}"
            f"&accounting_method=Accrual"
            f"&columns=tx_date,txn_type,doc_num,acc_name,acc_type,class_name,amt"
            f"&minorversion={QBO_MINOR_VERSION}"
        )
        logger.debug("GL Report URL: %s", url)

        res = await self._client.get(url, headers=self._auth_headers())

        if not res.is_success:
            if res.status_code == 401:
                raise AuthenticationError("QBO GL report request returned 401")
            err_body = res.text
            logger.error("QBO API Report Error (%d): %s", res.status_code, err_body)
            raise QboApiError(
                f"QBO API Report Error: {err_body}",
                status_code=res.status_code,
            )

        data: dict[str, Any] = res.json()
        logger.info("GL Report JSON received: %d bytes", len(res.content))
        return data

    async def fetch_active_classes(self) -> list[tuple[str, str]]:
        """Fetch all active QBO Classes.

        Returns a list of ``(Id, FullyQualifiedName)`` tuples.
        """
        query = quote("SELECT * FROM Class WHERE Active=true", safe="")
        url = (
            f"{self._base_url}/query"
            f"?query={query}"
            f"&minorversion={QBO_MINOR_VERSION}"
        )

        res = await self._client.get(url, headers=self._auth_headers())

        if not res.is_success:
            if res.status_code == 401:
                raise AuthenticationError("QBO class query returned 401")
            err_body = res.text
            logger.error("QBO Class Query Error (%d): %s", res.status_code, err_body)
            raise QboApiError(
                f"QBO Class Query Error: {err_body}",
                status_code=res.status_code,
            )

        data = res.json()
        class_arr = (
            data.get("QueryResponse", {}).get("Class") or []
        )

        result: list[tuple[str, str]] = []
        for cls in class_arr:
            cls_id = cls.get("Id", "")
            cls_name = cls.get("FullyQualifiedName", "")
            if cls_id and cls_name:
                result.append((cls_id, cls_name))

        logger.info("Fetched %d active QBO classes", len(result))
        return result

    async def fetch_transaction_by_id(
        self,
        tx_type: str,
        tx_id: str,
    ) -> dict[str, Any]:
        """Fetch a single transaction entity by its type and ID.

        The response is unwrapped from its PascalCase wrapper key using
        ``ENTITY_KEY_MAP`` so callers get the entity dict directly.
        """
        endpoint = tx_type.lower()
        url = (
            f"{self._base_url}/{endpoint}/{tx_id}"
            f"?minorversion={QBO_MINOR_VERSION}"
        )

        res = await self._client.get(url, headers=self._auth_headers())

        if not res.is_success:
            if res.status_code == 401:
                raise AuthenticationError(
                    f"QBO fetch {tx_type}/{tx_id} returned 401"
                )
            err_body = res.text
            logger.error(
                "QBO API Fetch Error (%s %s) [%d]: %s",
                tx_type, tx_id, res.status_code, err_body,
            )
            raise QboApiError(
                f"QBO API Fetch Error ({tx_type} {tx_id}): {err_body}",
                status_code=res.status_code,
            )

        json_data: dict[str, Any] = res.json()

        # QBO wraps the entity under a PascalCase key (e.g. "Bill", "Purchase").
        entity_key = ENTITY_KEY_MAP.get(tx_type, tx_type)

        entity = json_data.get(entity_key)
        if entity is None:
            # Fallback: QueryResponse wrapper (rare but observed)
            qr = json_data.get("QueryResponse", {})
            arr = qr.get(entity_key)
            if isinstance(arr, list) and arr:
                entity = arr[0]

        if entity is None:
            logger.warning(
                "Could not find entity key '%s' in QBO response for %s/%s, "
                "returning full response",
                entity_key, tx_type, tx_id,
            )
            return json_data

        return entity

    async def update_transaction_class(
        self,
        tx_type: str,
        sparse_update: dict[str, Any],
        request_id: str,
    ) -> dict[str, Any]:
        """POST a sparse update to QBO with retry for transient errors.

        Retry policy:
        - Retryable: 429 (rate-limit) and 5xx (server errors), plus network errors.
        - Fatal: 4xx (except 429) -- fail immediately.
        - Max 3 retries with exponential backoff (500ms, 1s, 2s) + jitter.
        """
        endpoint = tx_type.lower()
        url = (
            f"{self._base_url}/{endpoint}"
            f"?minorversion={QBO_MINOR_VERSION}"
        )

        headers = {
            **self._auth_headers(),
            "Content-Type": "application/json",
            "Request-Id": request_id,
        }

        logger.info(
            "Executing QBO Sparse Update (ReqID: %s): %s",
            request_id, tx_type,
        )

        last_error: str = ""

        for attempt in range(MAX_RETRIES + 1):
            # Exponential backoff with jitter (skip on first attempt)
            if attempt > 0:
                delay_ms = RETRY_BASE_DELAY_MS * (2 ** (attempt - 1))
                jitter_ms = random.randint(0, delay_ms // 2)
                total_delay = (delay_ms + jitter_ms) / 1000.0
                logger.info(
                    "Retry attempt %d/%d for QBO update (ReqID: %s), "
                    "sleeping %.2fs",
                    attempt, MAX_RETRIES, request_id, total_delay,
                )
                await asyncio.sleep(total_delay)

            # -- send request --
            try:
                res = await self._client.post(
                    url,
                    json=sparse_update,
                    headers=headers,
                )
            except httpx.HTTPError as exc:
                last_error = f"Request failed: {exc}"
                logger.warning(
                    "QBO request failed (attempt %d): %s", attempt + 1, exc,
                )
                continue  # network errors are retryable

            # -- success --
            if res.is_success:
                return res.json()

            status = res.status_code
            err_body = res.text

            # Transient errors (429 throttle, 5xx server) -> retry
            if status == 429 or status >= 500:
                logger.warning(
                    "QBO Transient Error (%d): %s. Will retry...",
                    status, err_body,
                )
                last_error = f"Transient QBO Error ({status}): {err_body}"
                continue

            # Fatal client errors (400, 401, 403, 404, etc.) -> fail immediately
            logger.error(
                "Fatal QBO Error (%d) for ReqID %s: %s",
                status, request_id, err_body,
            )
            raise QboApiError(
                f"Fatal QBO Error ({status}) - {err_body}",
                status_code=status,
            )

        # Exhausted all retries
        logger.error(
            "QBO update exhausted %d retries (ReqID: %s): %s",
            MAX_RETRIES, request_id, last_error,
        )
        raise QboApiError(last_error)

    async def fetch_company_info(self) -> dict[str, Any]:
        """Fetch raw CompanyInfo metadata for validation / diagnostics."""
        url = (
            f"{self._base_url}/companyinfo/{self._realm_id}"
            f"?minorversion={QBO_MINOR_VERSION}"
        )

        res = await self._client.get(url, headers=self._auth_headers())

        if not res.is_success:
            if res.status_code == 401:
                raise AuthenticationError("QBO CompanyInfo returned 401")
            err_body = res.text
            logger.error(
                "QBO CompanyInfo Fetch Error (%d): %s",
                res.status_code, err_body,
            )
            raise QboApiError(
                f"QBO CompanyInfo Fetch Error: {err_body}",
                status_code=res.status_code,
            )

        return res.json()
