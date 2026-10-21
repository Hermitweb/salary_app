//! 统一错误类型与 HTTP 响应映射。

use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use thiserror::Error;

/// 应用级错误。`Display` 文案即返回给前端的 `error` 字段。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Excel 文件不存在: {0}")]
    ExcelFileNotFound(String),

    #[error("员工表不存在: {0}")]
    EmployeeFileNotFound(String),

    #[error("{year} 年 {month} 月的工资表不存在")]
    SalaryFileNotFound { year: i32, month: u32 },

    #[error("{0} 在该月份没有工资记录")]
    EmployeeNotFound(String),

    #[error("认证失败: {0}")]
    AuthenticationFailed(String),

    #[error("账号或网络已被临时锁定，请在 {0} 秒后重试")]
    LoginLocked(u64),

    #[error("请求参数错误: {0}")]
    BadRequest(String),

    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("内部错误: {0}")]
    InternalError(String),
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::ExcelFileNotFound(_)
            | AppError::EmployeeFileNotFound(_)
            | AppError::SalaryFileNotFound { .. }
            | AppError::EmployeeNotFound(_) => StatusCode::NOT_FOUND,

            AppError::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
            AppError::LoginLocked(_) => StatusCode::TOO_MANY_REQUESTS,

            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::IoError(_) | AppError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(serde_json::json!({
            "ok": false,
            "error": self.to_string(),
        }))
    }
}
