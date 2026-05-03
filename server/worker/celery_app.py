from celery import Celery
from server.api.core.config import get_settings

settings = get_settings()

celery_app = Celery(
    "quant_worker",
    broker=settings.REDIS_URL,
    backend=settings.REDIS_URL,
    include=["server.worker.tasks"],
)

celery_app.conf.update(
    task_serializer="json",
    accept_content=["json"],
    result_serializer="json",
    timezone="Asia/Shanghai",
    enable_utc=True,
    task_track_started=True,
    task_time_limit=300,
    worker_prefetch_multiplier=1,
)
