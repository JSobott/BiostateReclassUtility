"""Diagnostics endpoints — connection status, company info, token management."""
import logging

from fastapi import APIRouter, HTTPException, Request

from ..models import SeedTokensRequest
from ..qbo_client import QboClient
from ..secrets import (
    KeychainError,
    get_oauth_data,
    store_oauth_data,
)

logger = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/diagnostics/status")
async def get_status(request: Request):
    """Return connection status and realm_id."""
    try:
        _access, _refresh, realm_id = get_oauth_data()
        return {"status": "connected", "realm_id": realm_id}
    except KeychainError:
        return {"status": "disconnected", "realm_id": None}
    except Exception as e:
        logger.error("Status check failed: %s", e)
        return {"status": "error", "realm_id": None}


@router.get("/api/diagnostics/company-info")
async def get_company_info(request: Request):
    """Fetch company info from QBO to verify connectivity."""
    try:
        access_token, refresh_token, realm_id = get_oauth_data()
    except KeychainError as e:
        raise HTTPException(status_code=401, detail="Not authenticated. Please connect to QuickBooks first.")

    try:
        client = QboClient(access_token, refresh_token, realm_id)
        info = await client.fetch_company_info()
        return info
    except Exception as e:
        logger.error("Failed to fetch company info: %s", e)
        raise HTTPException(status_code=500, detail=f"Failed to fetch company info: {e}")


@router.post("/api/diagnostics/refresh")
async def refresh_tokens(request: Request):
    """Force-refresh the OAuth access token."""
    try:
        access_token, refresh_token, realm_id = get_oauth_data()
    except KeychainError:
        raise HTTPException(status_code=401, detail="Not authenticated.")

    try:
        client = QboClient(access_token, refresh_token, realm_id)
        new_tokens = await client.refresh_tokens()
        store_oauth_data(new_tokens["access_token"], new_tokens["refresh_token"], realm_id)
        logger.info("Tokens refreshed successfully for realm %s", realm_id)
        return {"message": "Tokens refreshed successfully"}
    except Exception as e:
        logger.error("Token refresh failed: %s", e)
        raise HTTPException(status_code=500, detail=f"Token refresh failed: {e}")


@router.post("/api/diagnostics/seed-tokens")
async def seed_tokens(request: Request, body: SeedTokensRequest):
    """Manually seed OAuth tokens (for development / initial setup)."""
    try:
        store_oauth_data(body.access_token, body.refresh_token, body.realm_id)
        logger.info("Tokens seeded for realm %s", body.realm_id)
        return {"message": f"Tokens seeded for realm {body.realm_id}"}
    except Exception as e:
        logger.error("Failed to seed tokens: %s", e)
        raise HTTPException(status_code=500, detail=f"Failed to seed tokens: {e}")
