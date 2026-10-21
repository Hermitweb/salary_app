//! 口令哈希：PBKDF2-HMAC-SHA256，兼容历史明文口令。
//!
//! 员工表原本直接保存明文口令（与 Python 原版一致）。这里在保持兼容的前提下
//! 支持哈希口令：员工表里存的是 PHC 字符串格式的哈希，登录时按哈希校验。
//!
//! 哈希格式：`$pbkdf2-sha256$<迭代次数>$<盐(base64)>$<派生密钥(base64)>`。
//! 选这个格式有两个考虑：
//!
//! - 用的是 PHC 标准字符串格式，将来换成 Argon2id 之类的新算法时，
//!   存量数据的格式不用改，只需按前缀分派；
//! - Python 标准库的 `hashlib.pbkdf2_hmac` 能算出同样的结果，
//!   因此批量把员工表升级成哈希口令的脚本不需要任何第三方依赖。

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// 迭代次数。
///
/// 210000 是 PBKDF2-HMAC-SHA256 的常用推荐值：单次校验约 0.1~0.2 秒，
/// 既让离线爆破的代价足够高，又不至于让员工登录时明显卡顿。
const ITERATIONS: u32 = 210_000;

/// 允许的最大迭代次数。
///
/// 迭代次数是存在哈希串里的，如果无上限校验，攻击者只要把员工表里的迭代次数
/// 改成很大就能让服务在登录时疯狂空转。超出上限直接判定口令不匹配。
const MAX_ITERATIONS: u32 = 1_000_000;

/// 派生密钥长度（字节）。
const KEY_LEN: usize = 32;

/// 盐长度（字节）。
const SALT_LEN: usize = 16;

/// PHC 字符串格式的算法标识。
const ALGORITHM: &str = "pbkdf2-sha256";

/// 哈希串前缀。
const PREFIX: &str = "$pbkdf2-sha256$";

/// 常量时间字节比较，避免通过响应时间推测口令。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// PBKDF2-HMAC-SHA256（RFC 8018）。
fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(out_len);
    let mut block_index: u32 = 1;

    while result.len() < out_len {
        // U1 = PRF(P, S || INT_32_BE(i))
        let mut mac = HmacSha256::new_from_slice(password).expect("HMAC 接受任意长度的密钥");
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());

        let mut u = mac.finalize().into_bytes();
        let mut acc = u;

        // Uj = PRF(P, U(j-1))，T_i = U1 XOR U2 XOR ... XOR Uc
        for _ in 1..iterations {
            let mut mac = HmacSha256::new_from_slice(password).expect("HMAC 接受任意长度的密钥");
            mac.update(&u);
            u = mac.finalize().into_bytes();

            for (a, b) in acc.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }

        result.extend_from_slice(&acc);
        block_index += 1;
    }

    result.truncate(out_len);
    result
}

/// 判断存量口令是否已经是哈希值。
///
/// 只有形如 `$pbkdf2-sha256$...` 的值才按哈希校验，
/// 其余一律按明文比对，便于从明文平滑升级到哈希。
pub fn is_hashed(stored: &str) -> bool {
    stored.trim().starts_with(PREFIX)
}

/// 用指定盐与迭代次数生成哈希串。
///
/// 盐与迭代次数由调用方决定，便于测试里复现已知向量。
pub fn hash_with(plain: &str, salt: &[u8], iterations: u32) -> String {
    let derived = pbkdf2(plain.as_bytes(), salt, iterations, KEY_LEN);

    format!(
        "{PREFIX}{iterations}${}${}",
        STANDARD_NO_PAD.encode(salt),
        STANDARD_NO_PAD.encode(&derived)
    )
}

/// 生成一个带随机盐的哈希串。
pub fn hash(plain: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    hash_with(plain, &salt, ITERATIONS)
}

/// 校验口令。
///
/// 存量明文口令直接比对；哈希口令按 PHC 字符串解析后重新派生再比较。
pub fn verify(stored: &str, plain: &str) -> bool {
    let stored = stored.trim();

    if !is_hashed(stored) {
        return constant_time_eq(stored.as_bytes(), plain.as_bytes());
    }

    // "$pbkdf2-sha256$210000$<salt>$<hash>" 拆成 5 段
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 5 || !parts[0].is_empty() || parts[1] != ALGORITHM {
        return false;
    }

    let Ok(iterations) = parts[2].parse::<u32>() else {
        return false;
    };
    if iterations == 0 || iterations > MAX_ITERATIONS {
        return false;
    }

    let Ok(salt) = STANDARD_NO_PAD.decode(parts[3]) else {
        return false;
    };
    let Ok(expected) = STANDARD_NO_PAD.decode(parts[4]) else {
        return false;
    };
    if expected.is_empty() {
        return false;
    }

    let actual = pbkdf2(plain.as_bytes(), &salt, iterations, expected.len());
    constant_time_eq(&actual, &expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 由 Python 标准库 `hashlib.pbkdf2_hmac` 生成的已知向量：
    ///
    /// ```python
    /// import base64, hashlib
    /// salt = bytes(range(16))
    /// dk = hashlib.pbkdf2_hmac("sha256", b"salary2026", salt, 1000, 32)
    /// print(base64.b64encode(salt).decode().rstrip("="))
    /// print(base64.b64encode(dk).decode().rstrip("="))
    /// ```
    ///
    /// 跨实现校验能确保 Rust 侧与迁移脚本算出的结果完全一致。
    const PYTHON_SALT_B64: &str = "AAECAwQFBgcICQoLDA0ODw";
    const PYTHON_DK_B64: &str = "k0pV3/M6PVvjXe28rVH2TRuginHSC9515p2eOCXHvFg";

    #[test]
    fn matches_python_reference_vector() {
        let salt: Vec<u8> = STANDARD_NO_PAD.decode(PYTHON_SALT_B64).unwrap();
        assert_eq!(salt.len(), SALT_LEN);

        let derived = pbkdf2(b"salary2026", &salt, 1000, 32);
        assert_eq!(STANDARD_NO_PAD.encode(&derived), PYTHON_DK_B64);
    }

    #[test]
    fn hashes_and_verifies_a_password() {
        let hashed = hash("salary2026");
        assert!(is_hashed(&hashed));
        assert!(hashed.starts_with(PREFIX));
        assert!(verify(&hashed, "salary2026"));
        assert!(!verify(&hashed, "wrong-password"));
    }

    #[test]
    fn each_hash_uses_a_fresh_salt() {
        let first = hash("salary2026");
        let second = hash("salary2026");
        assert_ne!(first, second, "不同盐的哈希应当不同");
        assert!(verify(&first, "salary2026"));
        assert!(verify(&second, "salary2026"));
    }

    #[test]
    fn plaintext_passwords_still_work() {
        // 存量明文口令：登录行为不变
        assert!(!is_hashed("salary2026"));
        assert!(verify("salary2026", "salary2026"));
        assert!(!verify("salary2026", "salary2027"));
    }

    #[test]
    fn rejects_malformed_hash_strings() {
        assert!(!verify("$pbkdf2-sha256$", "salary2026"));
        assert!(!verify("$pbkdf2-sha256$abc$AA$BB", "salary2026"));
        assert!(!verify("$pbkdf2-sha256$0$AA$BB", "salary2026"));
        // 迭代次数超过上限：直接判失败，不去做昂贵的派生
        assert!(!verify(
            "$pbkdf2-sha256$999999999$AAECAwQFBgcICQoLDA0ODw$AAECAwQFBgcICQoLDA0ODw",
            "salary2026"
        ));
        assert!(!verify(
            "$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA",
            "salary2026"
        ));
    }

    #[test]
    fn ignores_surrounding_whitespace() {
        let hashed = hash("salary2026");
        assert!(verify(&format!("  {hashed}  "), "salary2026"));
        assert!(verify("  salary2026  ", "salary2026"));
    }
}
