"""Sync endpoint — trigger QBO transaction sync with SSE progress."""
import json
import logging

from fastapi import APIRouter, BackgroundTasks, Request
from sse_starlette.sse import EventSourceResponse

from ..models import SyncRequest
from ..sync import run_sync_job

logger = logging.getLogger(__name__)

router = APIRouter()


async def _perform_sync(db, broadcaster, start_date: str | None, end_date: str | None) -> None:
    """Background task: run the full sync pipeline."""
    async def progress_callback(msg: dict):
        await broadcaster.publish(msg)

    try:
        await broadcaster.publish({"status": "progress", "message": "Starting sync..."})
        await run_sync_job(db, start_date, end_date, progress_callback=progress_callback)
        await broadcaster.publish({"status": "done", "message": "Sync complete!"})
    except Exception as e:
        logger.error("Sync failed: %s", e)
        await broadcaster.publish({"status": "error", "message": str(e)})


@router.post("/api/sync", status_code=202)
async def start_sync(request: Request, body: SyncRequest, background_tasks: BackgroundTasks):
    """Kick off a QBO sync as a background task."""
    db = request.app.state.db
    broadcaster = request.app.state.broadcaster
    background_tasks.add_task(_perform_sync, db, broadcaster, body.start_date, body.end_date)
    return {"status": "accepted"}


@router.get("/api/sync-progress")
async def sync_progress(request: Request):
    """SSE stream for sync progress updates."""
    broadcaster = request.app.state.broadcaster
    queue = broadcaster.subscribe()

    async def event_generator():
        try:
            while True:
                message = await queue.get()
                yield {"event": "message", "data": json.dumps(message)}
                if message.get("status") in ("done", "error"):
                    break
        finally:
            broadcaster.unsubscribe(queue)

    return EventSourceResponse(event_generator())
