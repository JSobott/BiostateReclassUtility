"""OAuth2 authentication flow with Intuit / QuickBooks Online."""
import logging
import secrets
from urllib.parse import urlencode

from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import RedirectResponse

from ..config import OAUTH_AUTH_URL, OAUTH_REDIRECT_URI, OAUTH_SCOPE
from ..qbo_client import QboClient
from ..secrets import delete_oauth_data, get_client_credentials, store_oauth_data

logger = logging.getLogger(__name__)

router = APIRouter()


@router.get("/auth/login")
async def login(request: Request):
    """Redirect user to Intuit OAuth authorization page."""
    try:
        client_id, _client_secret = get_client_credentials()
    except Exception as e:
        logger.error("Failed to retrieve client credentials: %s", e)
        raise HTTPException(
            status_code=500,
            detail="Client credentials not configured. Seed them via /api/diagnostics/seed-tokens first.",
        )

    # Generate CSRF state token
    state = secrets.token_urlsafe(32)
    request.app.state.oauth_state = state

    params = urlencode({
        "client_id": client_id,
        "response_type": "code",
        "scope": OAUTH_SCOPE,
        "redirect_uri": OAUTH_REDIRECT_URI,
        "state": state,
    })

    auth_url = f"{OAUTH_AUTH_URL}?{params}"
    logger.info("Redirecting to Intuit OAuth: %s", auth_url)
    return RedirectResponse(url=auth_url)


@router.get("/callback")
async def oauth_callback(request: Request, code: str, realmId: str, state: str):
    """Handle OAuth callback from Intuit. Exchange code for tokens."""
    # CSRF verification — consume state with take pattern
    expected_state = request.app.state.oauth_state
    request.app.state.oauth_state = None  # consume (take pattern)

    if not expected_state:
        logger.error("OAuth callback received but no state was expected (double callback?)")
        raise HTTPException(status_code=400, detail="No OAuth state pending. Please restart login.")

    if state != expected_state:
        logger.error("CSRF state mismatch: expected=%s, got=%s", expected_state, state)
        raise HTTPException(status_code=403, detail="CSRF state mismatch. Please restart login.")

    # Exchange authorization code for tokens
    try:
        client_id, client_secret = get_client_credentials()
        temp_client = QboClient("", "", realmId)
        tokens = await temp_client.exchange_code(code, client_id, client_secret, OAUTH_REDIRECT_URI)

        access_token = tokens["access_token"]
        refresh_token = tokens["refresh_token"]

        store_oauth_data(access_token, refresh_token, realmId)
        logger.info("OAuth tokens stored for realm %s", realmId)

        # Redirect to frontend
        return RedirectResponse(url="http://127.0.0.1:5173")

    except Exception as e:
        logger.error("OAuth code exchange failed: %s", e)
        raise HTTPException(status_code=500, detail=f"Token exchange failed: {e}")


@router.get("/auth/disconnect")
async def disconnect(request: Request):
    """Clear stored OAuth tokens."""
    try:
        delete_oauth_data()
        request.app.state.oauth_state = None
        logger.info("OAuth tokens cleared")
        return {"status": "disconnected"}
    except Exception as e:
        logger.error("Failed to clear OAuth tokens: %s", e)
        raise HTTPException(status_code=500, detail="Failed to clear tokens")
