"""Classification rules CRUD endpoints."""
import logging

from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import JSONResponse

from ..config import ALLOWED_RULE_FIELDS
from ..models import ClassificationRule, DeleteRuleRequest

logger = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/rules", response_model=list[ClassificationRule])
async def get_rules(request: Request):
    """Return all classification rules."""
    db = request.app.state.db
    try:
        return await db.get_rules()
    except Exception as e:
        logger.error("Failed to fetch rules: %s", e)
        raise HTTPException(status_code=500, detail="Failed to fetch rules")


@router.post("/api/rules", status_code=201)
async def create_rule(request: Request, body: ClassificationRule):
    """Create a new classification rule."""
    if body.condition_field not in ALLOWED_RULE_FIELDS:
        raise HTTPException(
            status_code=400,
            detail=f"Invalid condition_field '{body.condition_field}'. Allowed: {ALLOWED_RULE_FIELDS}",
        )

    db = request.app.state.db
    try:
        await db.insert_rule(body)
        return {"status": "created"}
    except Exception as e:
        logger.error("Failed to create rule: %s", e)
        raise HTTPException(status_code=500, detail="Failed to create rule")


@router.post("/api/rules/delete")
async def delete_rule(request: Request, body: DeleteRuleRequest):
    """Delete a classification rule by id."""
    db = request.app.state.db
    try:
        await db.delete_rule(body.id)
        return {"status": "deleted"}
    except Exception as e:
        logger.error("Failed to delete rule %d: %s", body.id, e)
        raise HTTPException(status_code=500, detail="Failed to delete rule")
