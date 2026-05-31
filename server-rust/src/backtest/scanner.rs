//! 市场扫描器 — 通过 Python 沙箱批量执行用户策略，输出评分排序列表

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;

#[derive(Debug, Serialize)]
pub struct ScanResultItem {
    pub symbol: String,
    pub name: String,
    pub score: Decimal,
    pub total_return: Decimal,
    pub annual_return: Decimal,
    pub sharpe_ratio: Decimal,
    pub max_drawdown: Decimal,
    pub total_trades: u64,
    pub final_value: Decimal,
}

/// 扫描进度信息
pub struct ScanProgressInfo {
    pub progress: f64,
    pub message: String,
    pub elapsed_secs: Option<f64>,
    pub eta_secs: Option<f64>,
}

/// 扫描全市场标的（通过 Python 沙箱批量执行用户策略）
///
/// `progress_tx` 用于实时推送扫描进度（含 ETA）给调用方。
pub async fn scan_market(
    _db: &PgPool,
    ts_db: &PgPool,
    strategy_code: &str,
    symbols_input: &[String],
    exchange: &str,
    top_n: usize,
    score_threshold: Decimal,
    start_date: &str,
    end_date: &str,
    initial_cash: f64,
    progress_tx: Option<tokio::sync::mpsc::Sender<ScanProgressInfo>>,
    exclude_st: bool,
) -> Result<(Vec<ScanResultItem>, usize), AppError> {
    info!("Starting market scan with user strategy, top_n={}", top_n);

    // 1. 如果传入了具体标的就用传入的，否则从 stock_basic 加载全部
    let (symbols, name_map) = if !symbols_input.is_empty() {
        let names: std::collections::HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
            "SELECT symbol, name FROM stock_basic WHERE symbol = ANY($1)",
        )
        .bind(symbols_input)
        .fetch_all(ts_db)
        .await
        .map(|rows| rows.into_iter().collect())
        .unwrap_or_default();
        (symbols_input.to_vec(), names)
    } else {
        // 按市场过滤: cn=A股(6位数字), hk=港股(5位数字), us=美股(纯字母)
        let (sql, filter_desc) = match exchange {
            "hk" => {
                if exclude_st {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[0-9]{5}$' AND name NOT LIKE '%ST%' ORDER BY symbol", "港股(排除ST)")
                } else {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[0-9]{5}$' ORDER BY symbol", "港股")
                }
            },
            "us" => {
                if exclude_st {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[A-Z]+$' AND name NOT LIKE '%ST%' ORDER BY symbol", "美股(排除ST)")
                } else {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[A-Z]+$' ORDER BY symbol", "美股")
                }
            },
            _    => {
                if exclude_st {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[0-9]{6}$' AND name NOT LIKE '%ST%' ORDER BY symbol", "A股(排除ST)")
                } else {
                    ("SELECT symbol, name FROM stock_basic WHERE is_active = TRUE AND symbol ~ '^[0-9]{6}$' ORDER BY symbol", "A股")
                }
            },
        };
        let stocks: Vec<(String, String)> = sqlx::query_as(sql)
            .fetch_all(ts_db)
            .await
            .map_err(|e| AppError::Database(e))?;
        info!("Market filter '{}': {} stocks found", filter_desc, stocks.len());
        let syms: Vec<String> = stocks.iter().map(|(s, _)| s.clone()).collect();
        let names: std::collections::HashMap<String, String> = stocks.into_iter().collect();
        (syms, names)
    };

    info!("Scan: {} active stocks, calling Python batch sandbox", symbols.len());

    // 2. 调用 Python 沙箱批量扫描
    let batch_result = run_batch_scan_python(
        strategy_code,
        &symbols,
        start_date,
        end_date,
        initial_cash,
        progress_tx,
    ).await?;

    // 3. 解析结果
    let mut results: Vec<ScanResultItem> = Vec::new();
    let mut total_from_python = 0usize;
    if let Some(items) = batch_result.get("results").and_then(|v| v.as_array()) {
        total_from_python = items.len();
        for item in items {
            let symbol = item.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let total_return = item.get("total_return").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let annual_return = item.get("annual_return").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let sharpe = item.get("sharpe_ratio").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let dd = item.get("max_drawdown").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let trades = item.get("total_trades").and_then(|v| v.as_u64()).unwrap_or(0);
            let final_value = item.get("final_value").and_then(|v| v.as_f64()).unwrap_or(0.0);

            let score_dec = Decimal::from_f64_retain(score).unwrap_or(dec!(0));
            if score_dec >= score_threshold {
                results.push(ScanResultItem {
                    symbol: symbol.clone(),
                    name: name_map.get(&symbol).cloned().unwrap_or_default(),
                    score: score_dec,
                    total_return: Decimal::from_f64_retain(total_return).unwrap_or(dec!(0)),
                    annual_return: Decimal::from_f64_retain(annual_return).unwrap_or(dec!(0)),
                    sharpe_ratio: Decimal::from_f64_retain(sharpe).unwrap_or(dec!(0)),
                    max_drawdown: Decimal::from_f64_retain(dd).unwrap_or(dec!(0)),
                    total_trades: trades,
                    final_value: Decimal::from_f64_retain(final_value).unwrap_or(dec!(0)),
                });
            }
        }
    }

    let total_scanned = total_from_python;
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(top_n);

    info!("Scan complete: {} suitable stocks out of {} scanned ({} passed threshold)", results.len(), total_scanned, total_scanned - (total_scanned - results.len()));
    Ok((results, total_scanned))
}

/// 直接调用 Python 进程执行批量扫描
async fn run_batch_scan_python(
    strategy_code: &str,
    symbols: &[String],
    start_date: &str,
    end_date: &str,
    initial_cash: f64,
    progress_tx: Option<tokio::sync::mpsc::Sender<ScanProgressInfo>>,
) -> Result<serde_json::Value, AppError> {
    use tokio::io::{AsyncWriteExt, AsyncBufReadExt};

    let runner_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("sandbox_runner.py")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("sandbox_runner.py"));

    let input = serde_json::json!({
        "action": "batch_scan",
        "code": strategy_code,
        "symbols": symbols,
        "start_date": start_date,
        "end_date": end_date,
        "initial_cash": initial_cash,
        "period": "1d",
    });

    let mut child = tokio::process::Command::new("python3")
        .arg(runner_path.to_string_lossy().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AppError::BadRequest(format!("Failed to spawn Python: {}", e)))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(input.to_string().as_bytes()).await
            .map_err(|e| AppError::BadRequest(format!("Failed to write to Python stdin: {}", e)))?;
        stdin.write_all(b"\n").await
            .map_err(|e| AppError::BadRequest(format!("Failed to write newline: {}", e)))?;
    }
    drop(child.stdin.take());

    // Read stderr in a separate task for progress reporting + ETA forwarding
    let stderr = child.stderr.take().unwrap();
    let progress_tx_clone = progress_tx;
    let stderr_task = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        if let Ok(progress) = serde_json::from_str::<serde_json::Value>(trimmed) {
                            if let Some(msg) = progress.get("msg").and_then(|v| v.as_str()) {
                                let pct = progress.get("progress").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let elapsed = progress.get("elapsed").and_then(|v| v.as_f64());
                                let eta = progress.get("eta_seconds").and_then(|v| v.as_f64());
                                info!("Scan progress: {}% - {} (elapsed: {:?}s, ETA: {:?}s)", pct, msg, elapsed, eta);
                                if let Some(ref tx) = progress_tx_clone {
                                    let _ = tx.send(ScanProgressInfo {
                                        progress: pct,
                                        message: msg.to_string(),
                                        elapsed_secs: elapsed,
                                        eta_secs: eta,
                                    }).await;
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(1800),
        child.wait_with_output(),
    ).await
    .map_err(|_| AppError::BadRequest("Scan timeout (1800s)".to_string()))?
    .map_err(|e| AppError::BadRequest(format!("Python execution failed: {}", e)))?;

    // Wait for stderr reader to finish
    let _ = stderr_task.await;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("{}");
    serde_json::from_str(first_line)
        .map_err(|e| AppError::BadRequest(format!("Failed to parse scan result: {} - raw: {}", e, &first_line[..first_line.len().min(200)])))
}
