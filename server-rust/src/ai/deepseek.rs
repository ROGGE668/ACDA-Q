//! DeepSeek API 客户端
//!
//! 调用 DeepSeek Chat API 生成策略代码，兼容 OpenAI 格式。

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info};

use crate::error::AppError;

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";
const DEFAULT_MODEL: &str = "deepseek-chat";
const DEFAULT_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl DeepSeekClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key: api_key.into(),
            base_url: DEEPSEEK_API_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        let mut url = url.into();
        if !url.ends_with("/chat/completions") {
            url = url.trim_end_matches('/').to_string() + "/chat/completions";
        }
        self.base_url = url;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// 生成策略代码
    pub async fn generate_strategy(
        &self,
        prompt: &str,
    ) -> Result<(String, Option<u32>), AppError> {
        let system_prompt = r#"
你是一个A股量化交易策略生成助手。根据用户描述生成Python策略代码。

严格规则：
1. 代码总长度不超过15000字符
2. 不要写任何注释（包括#注释和文档字符串），注释会导致运行错误
3. 只生成 class Strategy(BaseStrategy) 类，不要定义 BaseStrategy（框架已提供）
4. 必须实现 on_bar(self, context, bar_group) 方法
5. bar_group 只包含当前交易日的数据（每个symbol一行），不要用它计算均线
6. 计算均线等需要历史数据时，必须用 context.history(symbol, lookback) 获取最近N天的close列表
7. 买入: context.buy(symbol, percent=0.1)，percent 为资金百分比
8. 卖出: context.sell(symbol, percent=1.0)，percent 为持仓百分比
9. 持仓查询: context.positions.get(symbol, 0) 返回持仓数量
10. 策略参数用 self.params.get("参数名", 默认值) 获取，参数名必须使用中文
11. 只用 numpy、pandas 和标准库
12. 只输出 Python 代码，不要 markdown 代码块标记，不要任何解释文字

参数名中文示例：
- 均线周期类: "短期均线周期", "长期均线周期"
- 阈值类: "买入阈值", "卖出阈值", "止损比例"
- 仓位类: "单笔仓位比例", "最大持仓数量"
- 其他: "回看天数", "冷却天数"

示例:
class Strategy(BaseStrategy):
    def on_bar(self, context, bar_group):
        for symbol in bar_group["symbol"].unique():
            short_period = self.params.get("短期均线周期", 5)
            long_period = self.params.get("长期均线周期", 20)
            history = context.history(symbol, long_period + 1)
            if len(history) < long_period + 1:
                continue
            closes = [h["close"] for h in history]
            sma_s = sum(closes[-short_period:]) / short_period
            sma_l = sum(closes[-long_period:]) / long_period
            holding = context.positions.get(symbol, 0) > 0
            if not holding and sma_s > sma_l:
                context.buy(symbol, percent=0.95)
            elif holding and sma_s < sma_l:
                context.sell(symbol, percent=1.0)
"#;

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
            ],
            temperature: 0.7,
            max_tokens: 4096,
        };

        debug!("Sending request to DeepSeek API");

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("DeepSeek API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("DeepSeek API error: {} - {}", status, body);
            return Err(AppError::Internal(format!(
                "DeepSeek API returned {}: {}",
                status, body
            )));
        }

        let completion: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse DeepSeek response: {}", e)))?;

        let choice = completion
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("Empty response from DeepSeek".to_string()))?;

        let code = self.extract_code(&choice.message.content);
        let tokens = completion.usage.map(|u| u.total_tokens);

        info!(
            "Strategy generated, tokens used: {:?}, finish_reason: {:?}",
            tokens, choice.finish_reason
        );

        Ok((code, tokens))
    }

    /// 从响应中提取 Python 代码（去除 markdown fence）
    fn extract_code(&self, content: &str) -> String {
        let trimmed = content.trim();

        // Remove ```python ... ``` fences
        if trimmed.starts_with("```python") {
            return trimmed
                .trim_start_matches("```python")
                .trim_end_matches("```")
                .trim()
                .to_string();
        }

        if trimmed.starts_with("```") {
            return trimmed
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
                .to_string();
        }

        trimmed.to_string()
    }

    /// 从策略代码中提取参数定义（简单正则式解析）
    pub fn extract_params(&self,
        code: &str,
    ) -> Vec<ParamDefinition> {
        let mut params = Vec::new();

        // Simple regex-like parsing for self.params.get("key", default)
        for line in code.lines() {
            let line = line.trim();
            if line.contains("self.params.get(") {
                if let Some(start) = line.find("\"") {
                    let rest = &line[start + 1..];
                    if let Some(end) = rest.find("\"") {
                        let key = &rest[..end];

                        // Try to infer type from default value
                        let default = if let Some(comma_pos) = rest[end..].find(",") {
                            let after_comma = rest[end + comma_pos + 1..].trim();
                            after_comma
                                .trim_end_matches(")")
                                .trim()
                                .to_string()
                        } else {
                            "null".to_string()
                        };

                        let param_type = infer_type(&default);

                        params.push(ParamDefinition {
                            name: key.to_string(),
                            param_type,
                            default,
                            description: None,
                        });
                    }
                }
            }
        }

        params
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub default: String,
    pub description: Option<String>,
}

fn infer_type(default: &str) -> String {
    if default.parse::<i64>().is_ok() {
        "int".to_string()
    } else if default.parse::<f64>().is_ok() {
        "float".to_string()
    } else if default == "True" || default == "False" || default == "true" || default == "false" {
        "bool".to_string()
    } else {
        "string".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_with_fence() {
        let client = DeepSeekClient::new("test-key");
        let code = client.extract_code("```python\nclass Strategy:\n    pass\n```");
        assert_eq!(code, "class Strategy:\n    pass");
    }

    #[test]
    fn test_extract_code_without_fence() {
        let client = DeepSeekClient::new("test-key");
        let code = client.extract_code("class Strategy:\n    pass");
        assert_eq!(code, "class Strategy:\n    pass");
    }

    #[test]
    fn test_infer_type() {
        assert_eq!(infer_type("10"), "int");
        assert_eq!(infer_type("1.5"), "float");
        assert_eq!(infer_type("True"), "bool");
        assert_eq!(infer_type("\"hello\""), "string");
    }
}
