//! 令牌兼容性测试：撤掉管理员概念后，升级前签发的令牌仍然可用。
//!
//! 单独成文件是因为 `integration_test.rs` 里 `use actix_web::test` 会遮蔽
//! 内置的 `#[test]` 属性，这里的用例不需要 HTTP，直接用内置测试属性更合适。

use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// 升级前签发的令牌在有效期内必须仍然可用。
///
/// 网页端撤掉管理员概念后，令牌载荷里不再有 is_admin 字段。
/// serde 默认忽略未知字段，因此老令牌不需要作废，员工不必重新登录。
#[test]
fn token_issued_before_admin_field_removal_is_still_accepted() {
    let key = b"integration-test-secret-key";
    let payload = json!({
        "employee_id": "001",
        "name": "张三",
        "dept": "工程部",
        "is_admin": true,
        "exp": chrono::Utc::now().timestamp() + 1800,
    })
    .to_string();

    // 按服务端的规则签名：base64url(载荷).hex(HMAC-SHA256(载荷))
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 密钥应当合法");
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    let token = format!(
        "{}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.as_bytes()),
        signature
    );

    let session = salary_app_backend::auth::verify_session_token(&token, key)
        .expect("旧令牌应当仍然可以解析");
    assert_eq!(session.employee_id, "001");
    assert_eq!(session.name.as_deref(), Some("张三"));
    assert_eq!(session.dept.as_deref(), Some("工程部"));
}
