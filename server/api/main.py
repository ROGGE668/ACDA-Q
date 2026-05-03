import json
import time
import uuid
from contextlib import asynccontextmanager

import os
from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import PlainTextResponse
from fastapi.staticfiles import StaticFiles
from prometheus_client import generate_latest, CONTENT_TYPE_LATEST, Counter, Histogram
import redis.asyncio as redis

from server.api.routers import auth, strategies, backtest, ai, market, ws, subscription, admin
from server.api.core.database import engine
from server.api.core.config import get_settings
from server.api.core.logging import configure_logging, get_logger
from server.worker.celery_app import celery_app
from sqlalchemy import text

configure_logging()
settings = get_settings()
logger = get_logger(__name__)

# Prometheus metrics
REQUEST_COUNT = Counter("http_requests_total", "Total HTTP requests", ["method", "endpoint", "status_code"])
REQUEST_DURATION = Histogram("http_request_duration_seconds", "HTTP request duration", ["method", "endpoint"])


def _parse_cors_origins() -> list[str]:
    try:
        origins = json.loads(settings.CORS_ORIGINS)
        if isinstance(origins, list):
            return origins
    except json.JSONDecodeError:
        pass
    # fallback: comma-separated string
    return [o.strip() for o in settings.CORS_ORIGINS.split(",") if o.strip()]


@asynccontextmanager
async def lifespan(app: FastAPI):
    logger.info("api_startup")
    yield
    await engine.dispose()
    logger.info("api_shutdown")


app = FastAPI(
    title=settings.APP_NAME,
    version="1.0.0",
    lifespan=lifespan,
)

# Request ID middleware
@app.middleware("http")
async def request_id_middleware(request: Request, call_next):
    request_id = request.headers.get("X-Request-ID") or str(uuid.uuid4())
    request.state.request_id = request_id

    start = time.time()
    response = await call_next(request)
    duration = time.time() - start

    # Attach request_id to response
    response.headers["X-Request-ID"] = request_id

    # Prometheus metrics
    path = request.url.path
    method = request.method
    status = str(response.status_code)
    REQUEST_COUNT.labels(method=method, endpoint=path, status_code=status).inc()
    REQUEST_DURATION.labels(method=method, endpoint=path).observe(duration)

    # Structured log
    logger.info(
        "request",
        method=method,
        path=path,
        status_code=status,
        duration_ms=round(duration * 1000, 2),
        request_id=request_id,
        client_ip=request.client.host if request.client else None,
    )

    return response


# CORS
origins = _parse_cors_origins()
app.add_middleware(
    CORSMiddleware,
    allow_origins=origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Routers
app.include_router(auth.router, prefix="/api/v1/auth", tags=["Auth"])
app.include_router(strategies.router, prefix="/api/v1/strategies", tags=["Strategies"])
app.include_router(backtest.router, prefix="/api/v1/backtests", tags=["Backtests"])
app.include_router(ai.router, prefix="/api/v1/ai", tags=["AI"])
app.include_router(market.router, prefix="/api/v1/market", tags=["Market"])
app.include_router(ws.router, tags=["WebSocket"])
app.include_router(subscription.router, prefix="/api/v1", tags=["Subscription"])
app.include_router(admin.router, prefix="/api/v1/admin", tags=["Admin"])

# Serve admin dashboard static files
static_dir = os.path.join(os.path.dirname(__file__), "../static")
if os.path.isdir(static_dir):
    app.mount("/admin", StaticFiles(directory=os.path.join(static_dir, "admin"), html=True), name="admin")


@app.get("/health")
async def health_check():
    checks = {}
    status_code = 200

    # DB check
    try:
        async with engine.connect() as conn:
            await conn.execute(text("SELECT 1"))
        checks["database"] = "ok"
    except Exception as e:
        checks["database"] = f"error: {e}"
        status_code = 503

    # Redis check
    try:
        r = redis.from_url(settings.REDIS_URL, decode_responses=True)
        await r.ping()
        await r.close()
        checks["redis"] = "ok"
    except Exception as e:
        checks["redis"] = f"error: {e}"
        status_code = 503

    # Celery check
    try:
        inspect = celery_app.control.inspect()
        ping = inspect.ping()
        checks["celery"] = "ok" if ping else "no_workers"
        if not ping:
            status_code = 503
    except Exception as e:
        checks["celery"] = f"error: {e}"
        status_code = 503

    return {
        "status": "healthy" if status_code == 200 else "unhealthy",
        "checks": checks,
    }


@app.get("/metrics")
async def metrics():
    return PlainTextResponse(generate_latest(), media_type=CONTENT_TYPE_LATEST)
