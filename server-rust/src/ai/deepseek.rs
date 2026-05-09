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
        self.base_url = url.into();
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
You are a quantitative trading strategy generator for A-share (Chinese stock market).
Generate a Python strategy class that inherits from BaseStrategy and implements on_bar method.

Requirements:
- Use only numpy, pandas, and standard library
- Buy/sell via context.buy(symbol, amount) and context.sell(symbol, amount)
- Get historical data via context.history(symbol, field, lookback)
- Class must be named 'Strategy'
- Include proper docstring in Chinese

Output ONLY the Python code, no markdown fences.
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
