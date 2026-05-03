import asyncio
import json
from typing import Dict, List
from fastapi import APIRouter, WebSocket, WebSocketDisconnect
import redis.asyncio as redis

from server.api.core.config import get_settings

settings = get_settings()
router = APIRouter()

# In-memory connection manager (per-process)
# For multi-instance deployments, Redis pub/sub bridges messages across instances
_connections: Dict[str, List[WebSocket]] = {}
_redis_subscriber_task = None


class ConnectionManager:
    async def connect(self, job_id: str, websocket: WebSocket):
        await websocket.accept()
        _connections.setdefault(job_id, []).append(websocket)

    def disconnect(self, job_id: str, websocket: WebSocket):
        if job_id in _connections:
            try:
                _connections[job_id].remove(websocket)
            except ValueError:
                pass
            if not _connections[job_id]:
                del _connections[job_id]

    async def broadcast(self, job_id: str, message: dict):
        if job_id not in _connections:
            return
        text = json.dumps(message)
        dead = []
        for ws in _connections[job_id]:
            try:
                await ws.send_text(text)
            except Exception:
                dead.append(ws)
        for ws in dead:
            self.disconnect(job_id, ws)


manager = ConnectionManager()


async def _redis_listener():
    """Background task: subscribe to Redis channel and broadcast to WebSocket clients."""
    try:
        r = redis.from_url(settings.REDIS_URL, decode_responses=True)
        pubsub = r.pubsub()
        await pubsub.subscribe("backtest_updates")
        async for message in pubsub.listen():
            if message["type"] != "message":
                continue
            try:
                data = json.loads(message["data"])
                job_id = data.get("job_id")
                if job_id:
                    await manager.broadcast(job_id, data)
            except Exception:
                pass
    except Exception:
        # Redis unavailable — gracefully degrade, clients will fall back to polling
        pass


def start_redis_listener():
    global _redis_subscriber_task
    if _redis_subscriber_task is None:
        loop = asyncio.get_event_loop()
        _redis_subscriber_task = loop.create_task(_redis_listener())


@router.websocket("/ws/backtest/{job_id}")
async def backtest_websocket(websocket: WebSocket, job_id: str):
    start_redis_listener()
    await manager.connect(job_id, websocket)
    try:
        while True:
            # Keep connection alive, optionally handle client pings
            data = await websocket.receive_text()
            try:
                msg = json.loads(data)
                if msg.get("type") == "ping":
                    await websocket.send_text(json.dumps({"type": "pong"}))
            except Exception:
                pass
    except WebSocketDisconnect:
        manager.disconnect(job_id, websocket)
    except Exception:
        manager.disconnect(job_id, websocket)


def publish_backtest_update_sync(job_id: str, status: str, progress: float = None, message: str = None):
    """Synchronous version for use in Celery workers."""
    import redis as _redis
    payload = {"job_id": job_id, "status": status}
    if progress is not None:
        payload["progress"] = round(progress, 2)
    if message:
        payload["message"] = message
    try:
        r = _redis.from_url(settings.REDIS_URL, decode_responses=True)
        r.publish("backtest_updates", json.dumps(payload))
        r.close()
    except Exception:
        pass


async def publish_backtest_update(job_id: str, status: str, progress: float = None, message: str = None):
    """Called by Celery worker or API to publish a backtest status update."""
    payload = {"job_id": job_id, "status": status}
    if progress is not None:
        payload["progress"] = round(progress, 2)
    if message:
        payload["message"] = message

    # Broadcast to local connections immediately
    await manager.broadcast(job_id, payload)

    # Publish to Redis for cross-instance broadcast
    try:
        r = redis.from_url(settings.REDIS_URL, decode_responses=True)
        await r.publish("backtest_updates", json.dumps(payload))
        await r.close()
    except Exception:
        pass
