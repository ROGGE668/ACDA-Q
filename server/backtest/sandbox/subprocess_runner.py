"""
子进程隔离运行器：在独立进程中执行策略回测，实现资源限制和网络隔离。
兼容 Python 3.11.8

注:用 billiard(Celery 内置的 multiprocessing fork)而不是标准库 multiprocessing,
因为 Celery prefork worker 是 daemon 进程,标准 multiprocessing 不允许 daemon
进程创建子进程,会抛 "daemonic processes are not allowed to have children"。
billiard API 完全兼容 multiprocessing。
"""
import billiard as mp
import resource
import socket
from typing import Dict, Any


def _subprocess_target(queue: mp.Queue, config: Dict[str, Any]) -> None:
    """子进程入口：执行回测并将结果写入队列。"""
    try:
        # --- 网络隔离：禁用 socket ---
        socket.socket = lambda *a, **k: (_ for _ in ()).throw(
            PermissionError("Network access is disabled in strategy sandbox")
        )

        # --- 资源限制 ---
        try:
            # 内存限制 512MB（软限制/硬限制）
            resource.setrlimit(
                resource.RLIMIT_AS, (512 * 1024 * 1024, 512 * 1024 * 1024)
            )
        except (ValueError, OSError, AttributeError):
            # macOS 不支持 RLIMIT_AS，尝试 RLIMIT_RSS
            try:
                resource.setrlimit(
                    resource.RLIMIT_RSS, (512 * 1024 * 1024, 512 * 1024 * 1024)
                )
            except (ValueError, OSError, AttributeError):
                pass

        try:
            # CPU 时间限制 60 秒（软/硬）
            resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
        except (ValueError, OSError, AttributeError):
            pass

        # --- 解析配置 ---
        code = config["code"]
        symbols = config.get("symbols", [])
        start_date = config["start_date"]
        end_date = config["end_date"]
        initial_cash = config["initial_cash"]
        params = config.get("params", {})
        scope = config.get("scope", "single")
        commission = config.get("commission", 0.0003)
        slippage = config.get("slippage", 0.001)
        stamp_duty = config.get("stamp_duty", 0.0005)
        transfer_fee = config.get("transfer_fee", 0.00001)

        # Portfolio-specific
        weight_mode = config.get("weight_mode", "equal")
        custom_weights = config.get("custom_weights")
        rebalance_freq = config.get("rebalance_freq", "1M")

        # Scan-specific
        top_n = config.get("top_n", 50)
        score_threshold = config.get("score_threshold", 60.0)

        # --- 编译策略 ---
        from server.backtest.sandbox.executor import (
            compile_strategy_code,
            load_strategy_class,
            SecurityError,
            StrategyLoadError,
        )

        module = compile_strategy_code(code)
        strategy_cls = load_strategy_class(module)

        # --- 执行回测 ---
        if scope == "scan":
            from server.backtest.engine.scanner import MarketScanner

            scanner = MarketScanner(
                code=code,
                start_date=start_date,
                end_date=end_date,
                initial_cash=initial_cash,
                top_n=top_n,
                score_threshold=score_threshold,
            )
            scan_result = scanner.run(symbols=symbols, params=params)
            result = {
                "signals": [
                    {
                        "symbol": s.symbol,
                        "direction": s.direction,
                        "timestamp": s.timestamp,
                        "price": s.price,
                        "score": s.score,
                    }
                    for s in scan_result.signals
                ],
                "overall_performance": scan_result.overall_performance,
                "suitable_stocks": scan_result.suitable_stocks,
                "unsuitable_stocks": scan_result.unsuitable_stocks,
            }

        elif scope == "portfolio":
            from server.backtest.engine.portfolio import PortfolioEngine

            engine = PortfolioEngine(
                initial_cash=initial_cash,
                commission=commission,
                slippage=slippage,
                stamp_duty=stamp_duty,
                transfer_fee=transfer_fee,
                weight_mode=weight_mode,
                custom_weights=custom_weights,
                rebalance_freq=rebalance_freq,
            )
            engine.set_strategy(strategy_cls, params=params)
            result = engine.run(symbols, start_date, end_date)
            result["trades"] = [
                {
                    "symbol": t.symbol,
                    "amount": t.amount,
                    "price": round(t.price, 2),
                    "type": t.order_type.value,
                    "timestamp": t.timestamp.isoformat(),
                    "commission": round(t.commission, 4),
                    "pnl": round(t.pnl, 2),
                }
                for t in engine.broker.trades
            ]
            result["equity_curve"] = engine.equity_curve
            result["positions"] = {
                sym: {
                    "qty": qty,
                    "last_price": round(engine.broker._last_price.get(sym, 0), 2),
                }
                for sym, qty in engine.broker.positions.items()
            }

        else:
            from server.backtest.engine.core import BacktestEngine

            engine = BacktestEngine(
                initial_cash=initial_cash,
                commission=commission,
                slippage=slippage,
                stamp_duty=stamp_duty,
                transfer_fee=transfer_fee,
            )
            engine.set_strategy(strategy_cls, params=params)
            result = engine.run(symbols, start_date, end_date)
            result["trades"] = [
                {
                    "symbol": t.symbol,
                    "amount": t.amount,
                    "price": round(t.price, 2),
                    "type": t.order_type.value,
                    "timestamp": t.timestamp.isoformat(),
                    "commission": round(t.commission, 4),
                    "pnl": round(t.pnl, 2),
                }
                for t in engine.broker.trades
            ]
            result["equity_curve"] = engine.equity_curve
            result["positions"] = {
                sym: {
                    "qty": qty,
                    "last_price": round(engine.broker._last_price.get(sym, 0), 2),
                }
                for sym, qty in engine.broker.positions.items()
            }

        queue.put({"status": "success", "result": result})

    except (SecurityError, StrategyLoadError, SyntaxError) as exc:
        queue.put({"status": "security_error", "error": str(exc)})
    except Exception as exc:
        import traceback

        queue.put({"status": "error", "error": str(exc), "traceback": traceback.format_exc()})


def run_backtest_subprocess(config: Dict[str, Any], timeout: int = 300) -> Dict[str, Any]:
    """
    在独立子进程中执行回测，超时自动 kill。
    :param config: 回测配置字典
    :param timeout: 超时时间（秒）
    :return: 回测结果字典
    """
    ctx = mp.get_context("spawn")
    queue = ctx.Queue()
    process = ctx.Process(target=_subprocess_target, args=(queue, config))
    process.start()
    process.join(timeout=timeout)

    if process.is_alive():
        process.terminate()
        process.join(timeout=5)
        if process.is_alive():
            process.kill()
            process.join()
        raise TimeoutError("策略回测执行超时（300秒）")

    if not queue.empty():
        data = queue.get()
        if data["status"] == "security_error":
            from server.backtest.sandbox.executor import SecurityError

            raise SecurityError(data["error"])
        if data["status"] == "error":
            raise RuntimeError(f"{data['error']}\n{data.get('traceback', '')}")
        return data["result"]

    raise RuntimeError("子进程未返回任何结果")
