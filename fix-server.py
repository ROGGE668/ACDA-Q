import os
import sys

fixes = []

# 1. Fix celery_app.py - add broker_connection_retry_on_startup
fixes.append(("server/worker/celery_app.py", """celery_app.conf.update(
    task_serializer="json",
    accept_content=["json"],
    result_serializer="json",
    timezone="Asia/Shanghai",
    enable_utc=True,
    task_track_started=True,
    task_time_limit=300,
    worker_prefetch_multiplier=1,
)""", """celery_app.conf.update(
    task_serializer="json",
    accept_content=["json"],
    result_serializer="json",
    timezone="Asia/Shanghai",
    enable_utc=True,
    task_track_started=True,
    task_time_limit=300,
    worker_prefetch_multiplier=1,
    broker_connection_retry_on_startup=True,
)"""))

# 2. Fix dependencies.py - quota field typo
fixes.append(("server/api/dependencies.py", """        if used >= current_user.quota_ai_daily:
            raise HTTPException(status_code=429, detail="Daily backtest quota exceeded")""", """        if used >= current_user.quota_backtest_daily:
            raise HTTPException(status_code=429, detail="Daily backtest quota exceeded")"""))

# 3. Fix main.py - health check performance (cache connections)
fixes.append(("server/api/main.py", """    # Redis check
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
        status_code = 503""", """    # Redis check
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
        inspect = celery_app.control.inspect(timeout=2.0)
        ping = inspect.ping()
        checks["celery"] = "ok" if ping else "no_workers"
        if not ping:
            status_code = 503
    except Exception as e:
        checks["celery"] = f"error: {e}"
        status_code = 503"""))

# 4. Add __init__.py to empty dirs
empty_dirs = [
    "server/backtest/broker/__init__.py",
    "server/backtest/indicators/__init__.py",
    "server/api/services/__init__.py",
]

for fpath in empty_dirs:
    full = os.path.join("/Users/hong/Documents/ACDA-Q", fpath)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    if not os.path.exists(full):
        with open(full, "w") as f:
            f.write("")
        print(f"Created: {fpath}")

# Apply text fixes
for fpath, old, new in fixes:
    full = os.path.join("/Users/hong/Documents/ACDA-Q", fpath)
    with open(full, "r") as f:
        content = f.read()
    if old in content:
        content = content.replace(old, new)
        with open(full, "w") as f:
            f.write(content)
        print(f"Fixed: {fpath}")
    else:
        print(f"SKIP (pattern not found): {fpath}")

print("\nServer fixes applied.")
