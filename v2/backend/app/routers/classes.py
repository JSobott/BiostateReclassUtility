"""QBO Class list endpoint with auto-refresh on 401."""
import logging

from fastapi import APIRouter, HTTPException, Request

from ..qbo_client import QboClient
from ..secrets import get_oauth_data, store_oauth_data

logger = logging.getLogger(__name__)

router = APIRouter()


async def get_valid_qbo_client() -> QboClient:
    """Create a QboClient, auto-refreshing tokens on 401."""
    access_token, refresh_token, realm_id = get_oauth_data()
    client = QboClient(access_token, refresh_token, realm_id)

    # Probe with a lightweight call; refresh if stale
    try:
        await client.probe()
        return client
    except Exception as e:
        if "401" in str(e):
            logger.info("Access token expired, refreshing...")
            try:
                new_tokens = await client.refresh_tokens()
                store_oauth_data(
                    new_tokens["access_token"],
                    new_tokens["refresh_token"],
                    realm_id,
                )
                return QboClient(
                    new_tokens["access_token"],
                    new_tokens["refresh_token"],
                    realm_id,
                )
            except Exception as refresh_err:
                logger.error("Token refresh failed: %s", refresh_err)
                raise HTTPException(
                    status_code=401,
                    detail="OAuth token refresh failed. Please re-authenticate.",
                )
        raise


@router.get("/api/classes")
async def get_classes(request: Request):
    """Return list of [id, name] tuples for all QBO Classes."""
    try:
        client = await get_valid_qbo_client()
        classes = await client.fetch_classes()
        # Return as list of [id, name] tuples
        result = [
            [cls["Id"], cls["Name"]]
            for cls in classes
            if "Id" in cls and "Name" in cls
        ]
        return result
    except HTTPException:
        raise
    except Exception as e:
        logger.error("Failed to fetch QBO classes: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch QBO classes")
