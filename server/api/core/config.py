from pydantic_settings import BaseSettings
from pydantic import model_validator
from functools import lru_cache


class Settings(BaseSettings):
    APP_NAME: str = "Quant Investment API"
    DEBUG: bool = False

    # Database (must be set via .env or environment)
    DATABASE_URL: str = ""
    SYNC_DATABASE_URL: str = ""
    TIMESCALE_DATABASE_URL: str = ""

    # Redis
    REDIS_URL: str = ""

    # Security
    SECRET_KEY: str = ""
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 30
    REFRESH_TOKEN_EXPIRE_DAYS: int = 7
    ALGORITHM: str = "HS256"

    # CORS
    CORS_ORIGINS: str = "[\"*\"]"

    # Cookie
    COOKIE_SECURE: bool = False

    # AI (DeepSeek 为主，兼容 OpenAI 格式)
    DEEPSEEK_API_KEY: str = ""
    DEEPSEEK_BASE_URL: str = "https://api.deepseek.com"
    DEEPSEEK_MODEL: str = "deepseek-chat"
    AI_CODE_TIMEOUT: int = 30
    AI_MAX_TOKENS: int = 4096

    # Data
    # MinIO (optional, not used in MVP)

    # Backtest
    BACKTEST_COMMISSION: float = 0.0003
    BACKTEST_SLIPPAGE: float = 0.001
    BACKTEST_STAMP_DUTY: float = 0.0005  # 卖出印花税
    BACKTEST_TRANSFER_FEE: float = 0.00001  # 过户费（双向，沪市）

    class Config:
        env_file = ".env"

    @model_validator(mode="after")
    def check_security_settings(self):
        if not self.SECRET_KEY or len(self.SECRET_KEY) < 32:
            raise ValueError(
                "SECRET_KEY must be set and at least 32 characters long. "
                "Generate a secure key with: openssl rand -hex 32"
            )
        if self.ACCESS_TOKEN_EXPIRE_MINUTES < 1:
            raise ValueError("ACCESS_TOKEN_EXPIRE_MINUTES must be at least 1")
        return self


@lru_cache()
def get_settings() -> Settings:
    return Settings()
