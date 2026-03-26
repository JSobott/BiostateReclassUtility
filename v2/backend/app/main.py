"""BiostateReclassUtility v2 — FastAPI application entry point."""
import asyncio
import logging
from contextlib import asynccontextmanager
from typing import Optional

import uvicorn
from fastapi import FastAPI, Request, Response
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from .db import init_db

from .routers import (
    auth,
    classes,
    diagnostics,
    rules,
    sync_router,
    transactions,
    writeback,
)

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# SSE Broadcaster — fan-out via per-subscriber asyncio.Queue
# ---------------------------------------------------------------------------
class Broadcaster:
    """Simple pub/sub broadcaster using asyncio.Queue per subscriber."""

    def __init__(self) -> None:
        self._subscribers: list[asyncio.Queue] = []

    async def publish(self, message: dict) -> None:
        for queue in list(self._subscribers):
            try:
                queue.put_nowait(message)
            except asyncio.QueueFull:
                logger.warning("Broadcaster: dropping message for a slow subscriber")

    def subscribe(self) -> asyncio.Queue:
        queue: asyncio.Queue = asyncio.Queue(maxsize=256)
        self._subscribers.append(queue)
        return queue

    def unsubscribe(self, queue: asyncio.Queue) -> None:
        try:
            self._subscribers.remove(queue)
        except ValueError:
            logger.warning("Broadcaster: attempted to unsubscribe an unknown queue")


# ---------------------------------------------------------------------------
# Lifespan
# ---------------------------------------------------------------------------
@asynccontextmanager
async def lifespan(app: FastAPI):
    # Startup
    logger.info("Initialising database...")
    app.state.db = await init_db()
    app.state.broadcaster = Broadcaster()
    app.state.oauth_state: Optional[str] = None
    logger.info("Application started")
    yield
    # Shutdown
    logger.info("Shutting down...")


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------
app = FastAPI(title="BiostateReclassUtility", version="2.0.0", lifespan=lifespan)


# ---------------------------------------------------------------------------
# CORS
# ---------------------------------------------------------------------------
app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://127.0.0.1:5173", "http://localhost:5173"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# ---------------------------------------------------------------------------
# Security headers + method filter middleware
# ---------------------------------------------------------------------------
@app.middleware("http")
async def security_middleware(request: Request, call_next) -> Response:
    # Reject TRACE / CONNECT
    if request.method in ("TRACE", "CONNECT"):
        return JSONResponse(
            status_code=405,
            content={"detail": f"Method {request.method} not allowed"},
        )

    response: Response = await call_next(request)

    response.headers["Cache-Control"] = "no-cache, no-store"
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["X-Frame-Options"] = "DENY"
    response.headers["Referrer-Policy"] = "no-referrer"

    return response


# ---------------------------------------------------------------------------
# Mount routers
# ---------------------------------------------------------------------------
app.include_router(auth.router)
app.include_router(transactions.router)
app.include_router(writeback.router)
app.include_router(rules.router)
app.include_router(classes.router)
app.include_router(diagnostics.router)
app.include_router(sync_router.router)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    uvicorn.run(
        "app.main:app",
        host="127.0.0.1",
        port=3030,
        reload=True,
    )
