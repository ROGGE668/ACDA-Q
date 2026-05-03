"""
Celery Worker：真正执行回测任务。
兼容 Python 3.11.8
"""
import os
import json
import traceback
import time
import logging
from datetime import datetime, timezone
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from prometheus_client import Counter, Histogram

from server.worker.celery_app import celery_app
from server.api.core.config import get_settings
from server.api.models.models import BacktestJob, Strategy, BacktestCache
from server.backtest.sandbox.executor import SecurityError, StrategyLoadError
from server.backtest.sandbox.subprocess_runner import run_backtest_subprocess
from server.api.core.timescale import get_ts_engine
from server.api.routers.ws import publish_backtest_update_sync
from server.api.core.alerting import alert_manager
from sqlalchemy import text

logger = logging.getLogger(__name__)

settings = get_settings()

# Sync DB for Celery worker
sync_engine = create_engine(settings.SYNC_DATABASE_URL)
SessionLocal = sessionmaker(bind=sync_engine)

# Prometheus metrics
BACKTEST_DURATION = Histogram("backtest_duration_seconds", "Backtest execution duration", ["scope"])
BACKTEST_TOTAL = Counter("backtest_total", "Total backtests", ["status", "scope"])


# 报告文件根目录（绝对路径，与 api/routers/backtest.py 保持一致）
_REPORT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../reports"))


def _save_report(job_id: str, result: dict) -> str:
    """将回测结果保存为JSON报告（MVP阶段存本地，后续接入MinIO）"""
    os.makedirs(_REPORT_DIR, exist_ok=True)
    # job_id 为 UUID，理论上安全，但仍做基础校验
    safe_name = os.path.basename(f"{job_id}.json")
    report_path = os.path.join(_REPORT_DIR, safe_name)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, indent=2, default=str)
    return report_path


@celery_app.task(bind=True, max_retries=2)
def run_backtest_task(self, job_id: str, cache_hash: str = None, strategy_code: str = None):
    request_id = self.request.headers.get("request_id") if self.request.headers else None
    scope_label = "unknown"
    start_time = time.time()
    session = SessionLocal()
    try:
        job = session.query(BacktestJob).filter(BacktestJob.id == job_id).first()
        if not job:
            raise ValueError(f"Job {job_id} not found")

        # 优先使用传入的临时策略代码，否则从数据库查询
        code = strategy_code
        if not code:
            strategy = session.query(Strategy).filter(Strategy.id == job.strategy_id).first()
            if not strategy:
                raise ValueError(f"Strategy for job {job_id} not found")
            code = strategy.code

        scope_label = job.scope or "single"
        job.status = "running"
        job.started_at = datetime.now(timezone.utc)
        session.commit()
        publish_backtest_update_sync(job_id, "running", progress=0.1, message="开始执行回测")

        # 解析参数
        symbols = job.symbols or []
        if isinstance(symbols, str):
            symbols = [s.strip() for s in symbols.split(",") if s.strip()]

        # 全市场扫描：从 stock_basic 获取股票列表
        if job.scope == "scan" or not symbols:
            ts_engine = get_ts_engine()
            with ts_engine.connect() as conn:
                result = conn.execute(text(
                    "SELECT symbol FROM stock_basic WHERE is_active = TRUE ORDER BY symbol LIMIT 500"
                ))
                symbols = [row[0] for row in result]
            if not symbols:
                raise ValueError("No stocks available for market scan")

        start_date = job.start_date.strftime("%Y-%m-%d") if job.start_date else "2023-01-01"
        end_date = job.end_date.strftime("%Y-%m-%d") if job.end_date else "2023-12-31"
        initial_cash = float(job.initial_cash)
        params = job.params or {}

        # 构建子进程配置
        backtest_config = {
            "code": code,
            "symbols": symbols,
            "start_date": start_date,
            "end_date": end_date,
            "initial_cash": initial_cash,
            "params": params,
            "scope": job.scope or "single",
            "commission": settings.BACKTEST_COMMISSION,
            "slippage": settings.BACKTEST_SLIPPAGE,
            "stamp_duty": getattr(settings, "BACKTEST_STAMP_DUTY", 0.0005),
            "transfer_fee": getattr(settings, "BACKTEST_TRANSFER_FEE", 0.00001),
        }
        if job.scope == "portfolio":
            backtest_config.update({
                "weight_mode": params.get("weight_mode", "equal"),
                "custom_weights": params.get("custom_weights"),
                "rebalance_freq": params.get("rebalance_freq", "1M"),
            })
        elif job.scope == "scan":
            backtest_config.update({
                "top_n": params.get("top_n", 50),
                "score_threshold": params.get("score_threshold", 60.0),
            })

        # 在独立子进程中执行回测（资源限制 + 网络隔离）
        result = run_backtest_subprocess(backtest_config, timeout=300)

        # 保存报告文件
        report_path = _save_report(str(job.id), result)

        # 提取摘要用于数据库存储（ trades 和 equity_curve 存文件）
        if job.scope == "scan":
            summary = result.get("overall_performance", {})
            summary["suitable_count"] = len(result.get("suitable_stocks", []))
        else:
            summary = {k: v for k, v in result.items() if k not in ("trades", "equity_curve", "positions", "signals", "suitable_stocks", "unsuitable_stocks")}

        # 更新数据库
        job.status = "success"
        job.result_summary = summary
        job.result_report_path = report_path
        job.completed_at = datetime.now(timezone.utc)
        session.commit()

        # Metrics
        BACKTEST_DURATION.labels(scope=scope_label).observe(time.time() - start_time)
        BACKTEST_TOTAL.labels(status="success", scope=scope_label).inc()
        publish_backtest_update_sync(job_id, "success", progress=1.0, message="回测完成")

        # 写入结果缓存（如果提供了 cache_hash）
        if cache_hash:
            try:
                cache_entry = BacktestCache(
                    cache_hash=cache_hash,
                    strategy_id=job.strategy_id,
                    scope=job.scope,
                    symbols=job.symbols,
                    start_date=job.start_date,
                    end_date=job.end_date,
                    initial_cash=job.initial_cash,
                    params=job.params,
                    result_summary=summary,
                    result_report_path=report_path,
                    expires_at=datetime.now(timezone.utc) + __import__("datetime").timedelta(days=7),
                )
                session.merge(cache_entry)
                session.commit()
            except Exception as cache_exc:
                logger.warning("Failed to write backtest cache: %s", cache_exc)

        return {
            "job_id": job_id,
            "status": "success",
            "summary": summary,
        }

    except (SecurityError, StrategyLoadError, SyntaxError) as exc:
        # 策略代码问题，不重试
        job.status = "failed"
        job.error_message = f"策略代码错误: {str(exc)}"
        job.completed_at = datetime.now(timezone.utc)
        session.commit()
        BACKTEST_TOTAL.labels(status="failed", scope=scope_label).inc()
        publish_backtest_update_sync(job_id, "failed", progress=1.0, message=str(exc))
        alert_manager.record_failure("run_backtest_task", error=str(exc))
        return {
            "job_id": job_id,
            "status": "failed",
            "error": str(exc),
        }

    except TimeoutError as exc:
        job.status = "failed"
        job.error_message = "策略回测执行超时（300秒）"
        job.completed_at = datetime.now(timezone.utc)
        session.commit()
        BACKTEST_TOTAL.labels(status="failed", scope=scope_label).inc()
        publish_backtest_update_sync(job_id, "failed", progress=1.0, message="执行超时")
        alert_manager.record_failure("run_backtest_task", error="timeout")
        return {
            "job_id": job_id,
            "status": "failed",
            "error": str(exc),
        }

    except Exception as exc:
        tb = traceback.format_exc()
        session.rollback()
        job = session.query(BacktestJob).filter(BacktestJob.id == job_id).first()
        if job:
            if self.request.retries < self.max_retries:
                job.status = "pending"
                job.error_message = f"执行异常（将重试）: {str(exc)}"
            else:
                job.status = "failed"
                job.error_message = f"执行失败: {str(exc)}\n{tb}"
                job.completed_at = datetime.now(timezone.utc)
                publish_backtest_update_sync(job_id, "failed", progress=1.0, message=str(exc))
            session.commit()
        session.close()
        BACKTEST_TOTAL.labels(status="failed", scope=scope_label).inc()
        alert_manager.record_failure("run_backtest_task", error=str(exc))
        if self.request.retries < self.max_retries:
            raise self.retry(exc=exc, countdown=30)
        return {
            "job_id": job_id,
            "status": "failed",
            "error": str(exc),
        }

    finally:
        if session.is_active:
            session.close()
