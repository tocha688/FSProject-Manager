// GitHub 加速镜像源。
//
// 国内直连 github.com / objects.githubusercontent.com 常常很慢或超时。
// 这里维护一组公共镜像，更新时并发测速，用最快的那个。
//
// 注意：镜像只做「下载加速」，**不做完整性校验的替代**——
// 安装版仍由 Tauri updater 用 minisign 公钥验签，
// 便携版我们自己在解压后校验主程序是否存在。
// 因此镜像被投毒也无法通过签名校验（安装版）；便携版风险由 HTTPS + 域名白名单控制。
use serde::Serialize;

/// 一个镜像源定义。
pub struct Mirror {
    pub id: &'static str,
    pub label: &'static str,
    /// 前缀：把 https://github.com/... 变成 {prefix}https://github.com/...
    /// 为空表示直连（官方源）。
    pub prefix: &'static str,
}

/// 内置镜像列表，按常用程度排序；直连放第一个作为基线。
pub const MIRRORS: &[Mirror] = &[
    Mirror {
        id: "direct",
        label: "GitHub 直连",
        prefix: "",
    },
    Mirror {
        id: "gh-proxy-com",
        label: "gh-proxy.com",
        prefix: "https://gh-proxy.com/",
    },
    Mirror {
        id: "ghproxy-cfd",
        label: "ghproxy.cfd",
        prefix: "https://ghproxy.cfd/",
    },
    Mirror {
        id: "ghfast-top",
        label: "ghfast.top",
        prefix: "https://ghfast.top/",
    },
    Mirror {
        id: "ghproxy-net",
        label: "ghproxy.net",
        prefix: "https://ghproxy.net/",
    },
    Mirror {
        id: "ghproxy-cc",
        label: "ghproxy.cc",
        prefix: "https://ghproxy.cc/",
    },
    Mirror {
        id: "moeyy",
        label: "moeyy.cn",
        prefix: "https://github.moeyy.xyz/",
    },
    Mirror {
        id: "gitmirror",
        label: "gitmirror.com",
        prefix: "https://hub.gitmirror.com/",
    },
];

/// 把官方 URL 套到某个镜像上。
pub fn apply_mirror(prefix: &str, url: &str) -> String {
    if prefix.is_empty() {
        url.to_string()
    } else {
        format!("{prefix}{url}")
    }
}

/// 某个镜像的可达性测试结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorSpeed {
    pub id: String,
    pub label: String,
    /// 是否可用
    pub ok: bool,
    /// 往返耗时（毫秒）；失败时为 0
    pub elapsed_ms: u64,
    /// 失败原因（ok=false 时）
    pub error: String,
}

/// 从测速结果里挑最快的可用镜像。
/// 返回 None 表示全部不可用。
pub fn pick_fastest(results: &[MirrorSpeed]) -> Option<&MirrorSpeed> {
    results
        .iter()
        .filter(|r| r.ok)
        .min_by_key(|r| r.elapsed_ms)
}

/// 按"直连优先"策略挑选：若直连在阈值内够快就不折腾镜像。
///
/// 这样可以避免国内用户明明直连没问题，却被导向第三方镜像
/// （多一跳代理反而慢，且多一个信任点）。
pub fn pick_best(results: &[MirrorSpeed], direct_fast_ms: u64) -> Option<&MirrorSpeed> {
    let direct = results.iter().find(|r| r.id == "direct");
    if let Some(d) = direct {
        if d.ok && d.elapsed_ms <= direct_fast_ms {
            return Some(d);
        }
    }
    pick_fastest(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speed(id: &str, ok: bool, ms: u64) -> MirrorSpeed {
        MirrorSpeed {
            id: id.into(),
            label: id.into(),
            ok,
            elapsed_ms: ms,
            error: if ok { String::new() } else { "timeout".into() },
        }
    }

    #[test]
    fn apply_mirror_direct_is_identity() {
        let url = "https://github.com/a/b/releases/download/v1/x.exe";
        assert_eq!(apply_mirror("", url), url);
    }

    #[test]
    fn apply_mirror_prefixes() {
        let url = "https://github.com/a/b/releases/download/v1/x.exe";
        assert_eq!(
            apply_mirror("https://ghproxy.net/", url),
            format!("https://ghproxy.net/{url}")
        );
    }

    #[test]
    fn pick_fastest_ignores_failures() {
        let r = vec![speed("a", false, 1), speed("b", true, 300), speed("c", true, 120)];
        assert_eq!(pick_fastest(&r).map(|s| s.id.as_str()), Some("c"));
    }

    #[test]
    fn pick_fastest_none_when_all_fail() {
        let r = vec![speed("a", false, 0), speed("b", false, 0)];
        assert!(pick_fastest(&r).is_none());
    }

    #[test]
    fn pick_best_prefers_direct_when_fast_enough() {
        let r = vec![speed("direct", true, 200), speed("mirror", true, 80)];
        // 直连 200ms 在 500ms 阈值内 → 用直连，不折腾镜像
        assert_eq!(pick_best(&r, 500).map(|s| s.id.as_str()), Some("direct"));
    }

    #[test]
    fn pick_best_uses_mirror_when_direct_slow() {
        let r = vec![speed("direct", true, 3000), speed("mirror", true, 120)];
        assert_eq!(pick_best(&r, 500).map(|s| s.id.as_str()), Some("mirror"));
    }

    #[test]
    fn pick_best_uses_mirror_when_direct_fails() {
        let r = vec![speed("direct", false, 0), speed("mirror", true, 900)];
        assert_eq!(pick_best(&r, 500).map(|s| s.id.as_str()), Some("mirror"));
    }

    #[test]
    fn pick_best_none_when_nothing_works() {
        let r = vec![speed("direct", false, 0), speed("mirror", false, 0)];
        assert!(pick_best(&r, 500).is_none());
    }

    #[test]
    fn mirror_ids_are_unique() {
        let mut ids: Vec<&str> = MIRRORS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn mirrors_include_direct_and_have_valid_prefixes() {
        assert!(MIRRORS.iter().any(|m| m.prefix.is_empty()), "必须包含直连");
        for m in MIRRORS {
            if !m.prefix.is_empty() {
                assert!(
                    m.prefix.starts_with("https://") && m.prefix.ends_with('/'),
                    "{} 的前缀必须是 https 且以 / 结尾: {}",
                    m.id,
                    m.prefix
                );
            }
        }
    }
}
