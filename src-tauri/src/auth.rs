// 密码哈希：PBKDF2-HMAC-SHA256，纯 Rust 实现（无第三方加密库依赖）。
//
// 存储格式：pbkdf2_sha256$<迭代次数>$<salt_hex>$<hash_hex>
// 校验用常数时间比较，避免时序侧信道。
//
// 说明：这是本地应用锁，不是服务器认证系统。使用了足够高的迭代次数
// （默认 12 万次）让离线爆破代价变高，但真正的安全边界是"电脑被人拿到"。

const ITERATIONS: u32 = 120_000;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;

// ---------- SHA-256 ----------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = ipad.to_vec();
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

// ---------- PBKDF2 ----------

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, key_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(key_len);
    let mut block = 1u32;
    while out.len() < key_len {
        let mut msg = salt.to_vec();
        msg.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_sha256(password, &msg);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for i in 0..32 {
                t[i] ^= u[i];
            }
        }
        out.extend_from_slice(&t);
        block += 1;
    }
    out.truncate(key_len);
    out
}

// ---------- 随机 salt ----------

fn random_bytes(n: usize) -> Vec<u8> {
    #[cfg(windows)]
    {
        // 用 Windows CNG 的 RtlGenRandom（SystemFunction036）取系统熵
        #[link(name = "advapi32")]
        extern "system" {
            fn SystemFunction036(buf: *mut u8, len: u32) -> u8;
        }
        let mut buf = vec![0u8; n];
        // SAFETY: buf 有效且长度与传入的 len 一致
        let ok = unsafe { SystemFunction036(buf.as_mut_ptr(), n as u32) };
        if ok != 0 {
            return buf;
        }
    }
    // 非 Windows 或调用失败：退化为时间 + 进程 id 派生（本地应用可接受）
    let mut seed = Vec::new();
    seed.extend_from_slice(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .to_be_bytes(),
    );
    seed.extend_from_slice(&std::process::id().to_be_bytes());
    let mut out = Vec::new();
    let mut counter = 0u64;
    while out.len() < n {
        let mut m = seed.clone();
        m.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&sha256(&m));
        counter += 1;
    }
    out.truncate(n);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// ---------- 对外接口 ----------

/// 生成密码哈希串。
pub fn hash_password(password: &str) -> String {
    let salt = random_bytes(SALT_LEN);
    let key = pbkdf2_sha256(password.as_bytes(), &salt, ITERATIONS, KEY_LEN);
    format!(
        "pbkdf2_sha256${}${}${}",
        ITERATIONS,
        hex(&salt),
        hex(&key)
    )
}

/// 校验密码。格式非法或字段缺失一律返回 false（不 panic）。
pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2_sha256" {
        return false;
    }
    let iterations: u32 = match parts[1].parse() {
        Ok(v) if v > 0 => v,
        _ => return false,
    };
    let salt = match unhex(parts[2]) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let expected = match unhex(parts[3]) {
        Some(h) if !h.is_empty() => h,
        _ => return false,
    };
    let actual = pbkdf2_sha256(password.as_bytes(), &salt, iterations, expected.len());
    // 常数时间比较
    if actual.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in actual.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_known_vector() {
        // RFC 4231 Test Case 2
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn pbkdf2_known_vectors() {
        // RFC 6070 (SHA-256 变体见 RFC 7914 附录，此处用广泛引用的测试向量)
        assert_eq!(
            hex(&pbkdf2_sha256(b"password", b"salt", 1, 32)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            hex(&pbkdf2_sha256(b"password", b"salt", 2, 32)),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let stored = hash_password("correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &stored));
        assert!(!verify_password("wrong password", &stored));
    }

    #[test]
    fn same_password_gives_different_hashes() {
        // 每次都用新 salt
        let a = hash_password("abc");
        let b = hash_password("abc");
        assert_ne!(a, b);
        assert!(verify_password("abc", &a));
        assert!(verify_password("abc", &b));
    }

    #[test]
    fn stored_format_is_parseable() {
        let s = hash_password("x");
        let parts: Vec<&str> = s.split('$').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "pbkdf2_sha256");
        assert_eq!(parts[1].parse::<u32>().unwrap(), ITERATIONS);
        assert_eq!(parts[2].len(), SALT_LEN * 2);
        assert_eq!(parts[3].len(), KEY_LEN * 2);
    }

    #[test]
    fn malformed_stored_values_fail_safely() {
        for bad in [
            "",
            "plaintext",
            "pbkdf2_sha256$",
            "pbkdf2_sha256$abc$x$y",
            "pbkdf2_sha256$1000$zz$yy",
            "pbkdf2_sha256$0$aa$bb",
            "md5$1$aa$bb",
        ] {
            assert!(!verify_password("x", bad), "{bad} 应校验失败");
        }
    }

    #[test]
    fn unicode_password_works() {
        let s = hash_password("中文密码🔐");
        assert!(verify_password("中文密码🔐", &s));
        assert!(!verify_password("中文密码", &s));
    }

    #[test]
    fn empty_password_can_be_hashed_and_verified() {
        let s = hash_password("");
        assert!(verify_password("", &s));
        assert!(!verify_password("x", &s));
    }

    #[test]
    fn unhex_rejects_odd_length_and_bad_chars() {
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
        assert_eq!(unhex("0aff"), Some(vec![0x0a, 0xff]));
    }

    #[test]
    fn random_bytes_has_right_length_and_varies() {
        let a = random_bytes(16);
        let b = random_bytes(16);
        assert_eq!(a.len(), 16);
        assert_ne!(a, b, "两次随机应不同（概率上）");
    }
}
