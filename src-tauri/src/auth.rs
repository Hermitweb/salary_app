//! 会话令牌、登录限流与认证提取器。
//!
//! 令牌格式：`base64url(payload).hex(HMAC-SHA256(payload))`，
//! 载荷为 [`SessionToken`]，签名密钥保存在 `data/secret.key`。

use crate::models::{Employee, SessionToken};
use actix_web::dev::Payload;
use actix_web::http::header;
use actix_web::{Error, FromRequest, HttpRequest, web};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::future::{Ready, ready};
use std::sync::{Arc, RwLock};

type HmacSha256 = Hmac<Sha256>;

/// 登录限流器：同一 key 在时间窗内失败达到阈值即锁定。
///
/// key 约定：`ip:<地址>` 与 `user:<工号>`，两个维度独立计数、独立锁定。
#[derive(Clone, Debug)]
pub struct RateLimiter {
    /// 时间窗内允许的最大失败次数
    pub max_attempts: usize,
    /// 失败计数时间窗（秒）
    pub window_seconds: i64,
    /// 锁定时长（秒）
    pub lock_seconds: i64,
    failures: Arc<RwLock<HashMap<String, Vec<i64>>>>,
    locks: Arc<RwLock<HashMap<String, i64>>>,
}

impl RateLimiter {
    pub fn new(max_attempts: usize, window_seconds: i64, lock_seconds: i64) -> Self {
        Self {
            max_attempts,
            window_seconds,
            lock_seconds,
            failures: Arc::new(RwLock::new(HashMap::new())),
            locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 查询剩余锁定时间（秒）。未被锁定返回 `None`。
    pub fn remaining_lock(&self, key: &str) -> Option<u64> {
        let now = Utc::now().timestamp();
        let until = *self
            .locks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)?;

        if until > now {
            Some((until - now) as u64)
        } else {
            // 锁定已过期，顺手清理
            self.locks
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .remove(key);
            None
        }
    }

    /// 记录一次失败。返回 `true` 表示本次失败触发了锁定。
    pub fn record_failure(&self, key: &str) -> bool {
        let now = Utc::now().timestamp();
        let cutoff = now - self.window_seconds;

        let mut failures = self.failures.write().unwrap_or_else(|e| e.into_inner());
        let entry = failures.entry(key.to_string()).or_default();
        entry.retain(|&t| t >= cutoff);
        entry.push(now);

        if entry.len() >= self.max_attempts {
            let mut locks = self.locks.write().unwrap_or_else(|e| e.into_inner());
            locks.insert(key.to_string(), now + self.lock_seconds);
            true
        } else {
            false
        }
    }

    /// 登录成功后清除该 key 的失败计数与锁定。
    pub fn clear_failures(&self, key: &str) {
        self.failures
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        self.locks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }

    /// 当前累计失败次数（测试与诊断用）。
    #[cfg(test)]
    pub fn failure_count(&self, key: &str) -> usize {
        self.failures
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        // 15 分钟内失败 5 次，锁定 15 分钟
        Self::new(5, 15 * 60, 15 * 60)
    }
}

/// 认证上下文：签名密钥 + 限流器 + 令牌有效期。
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub secret_key: Vec<u8>,
    pub session_ttl_minutes: u64,
    pub rate_limiter: RateLimiter,
}

impl AuthContext {
    pub fn new(secret_key: Vec<u8>) -> Self {
        Self {
            secret_key,
            session_ttl_minutes: 30,
            rate_limiter: RateLimiter::default(),
        }
    }

    /// 为员工签发令牌。
    pub fn issue_token(&self, employee: &Employee) -> Result<String, String> {
        issue_session_token(employee, &self.secret_key, self.session_ttl_minutes)
    }
}

/// 计算载荷签名。
fn sign(secret_key: &[u8], payload: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(secret_key).map_err(|e| e.to_string())?;
    mac.update(payload);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// 签发会话令牌。
pub fn issue_session_token(
    employee: &Employee,
    secret_key: &[u8],
    ttl_minutes: u64,
) -> Result<String, String> {
    let payload = SessionToken {
        employee_id: employee.employee_id.clone(),
        name: employee.name.clone(),
        dept: employee.dept.clone(),
        exp: Utc::now().timestamp() + (ttl_minutes as i64) * 60,
    };

    let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let signature = sign(secret_key, json.as_bytes())?;

    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(json.as_bytes()),
        signature
    ))
}

/// 校验并解析会话令牌。
pub fn verify_session_token(token: &str, secret_key: &[u8]) -> Result<SessionToken, String> {
    let (encoded_payload, signature) = token.split_once('.').ok_or("令牌格式错误")?;

    let payload = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| "令牌编码错误")?;
    let expected = sign(secret_key, &payload)?;

    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err("令牌签名无效".to_string());
    }

    let session: SessionToken = serde_json::from_slice(&payload).map_err(|_| "令牌载荷错误")?;

    // `exp` 是有效期的右开边界：到达该时刻即视为过期
    if Utc::now().timestamp() >= session.exp {
        return Err("令牌已过期".to_string());
    }

    Ok(session)
}

/// 常量时间字节比较，避免通过响应时间推测签名。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 从请求头中取出 Bearer 令牌。
fn bearer_token(req: &HttpRequest) -> Option<String> {
    let raw = req
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();

    if raw.len() >= 7 && raw[..7].eq_ignore_ascii_case("bearer ") {
        Some(raw[7..].trim().to_string())
    } else {
        Some(raw.to_string())
    }
}

/// 从请求中解析出已认证的会话。
fn resolve_session(req: &HttpRequest) -> Result<SessionToken, Error> {
    let context = req
        .app_data::<web::Data<AuthContext>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("认证上下文未初始化"))?;

    let token = bearer_token(req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("缺少 Authorization 头"))?;

    verify_session_token(&token, &context.secret_key).map_err(actix_web::error::ErrorUnauthorized)
}

/// 认证提取器：校验 `Authorization: Bearer <token>`。
#[derive(Debug, Clone)]
pub struct AuthUser(pub SessionToken);

impl FromRequest for AuthUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(resolve_session(req).map(AuthUser))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn employee() -> Employee {
        Employee {
            employee_id: "001".to_string(),
            name: Some("张三".to_string()),
            dept: Some("工程部".to_string()),
        }
    }

    #[test]
    fn token_round_trip() {
        let key = b"test-secret-key";
        let token = issue_session_token(&employee(), key, 30).unwrap();
        let session = verify_session_token(&token, key).unwrap();

        assert_eq!(session.employee_id, "001");
        assert_eq!(session.name.as_deref(), Some("张三"));
        assert_eq!(session.dept.as_deref(), Some("工程部"));
    }

    #[test]
    fn token_rejects_wrong_signature() {
        let token = issue_session_token(&employee(), b"secret-a", 30).unwrap();
        assert!(verify_session_token(&token, b"secret-b").is_err());
    }

    #[test]
    fn token_rejects_tampered_payload() {
        let key = b"secret";
        let token = issue_session_token(&employee(), key, 30).unwrap();
        let (payload, signature) = token.split_once('.').unwrap();
        let tampered = format!("{payload}x.{signature}");

        assert!(verify_session_token(&tampered, key).is_err());
    }

    #[test]
    fn token_rejects_expired() {
        let key = b"secret";
        // TTL 为 0 表示立即过期
        let token = issue_session_token(&employee(), key, 0).unwrap();
        assert!(verify_session_token(&token, key).is_err());
    }

    #[test]
    fn rate_limiter_locks_after_threshold() {
        let limiter = RateLimiter::new(3, 60, 60);

        assert!(!limiter.record_failure("user:001"));
        assert!(!limiter.record_failure("user:001"));
        assert!(limiter.record_failure("user:001"));

        assert!(limiter.remaining_lock("user:001").is_some());
        assert_eq!(limiter.failure_count("user:001"), 3);

        limiter.clear_failures("user:001");
        assert!(limiter.remaining_lock("user:001").is_none());
    }

    #[test]
    fn rate_limiter_isolates_keys() {
        let limiter = RateLimiter::new(2, 60, 60);
        limiter.record_failure("user:001");
        limiter.record_failure("user:001");
        assert!(limiter.record_failure("user:001"));
        assert!(limiter.remaining_lock("user:002").is_none());
    }
}
