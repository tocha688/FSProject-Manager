use crate::db::ProjectRow;

/// 参与评分检索的一份项目文档。
#[derive(Debug, Clone)]
pub struct SearchDoc {
    pub id: i64,
    pub name: String,
    pub language: String,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub deps: Vec<String>,
    pub readme: Option<String>,
    pub commits: Vec<String>,
    pub last_commit_at: Option<i64>,
}

impl SearchDoc {
    pub fn from_row(row: &ProjectRow) -> Self {
        SearchDoc {
            id: row.id,
            name: row.name.clone(),
            language: row.language.clone(),
            tags: row.tags.clone(),
            description: row.description.clone(),
            notes: row.notes.clone(),
            deps: row.deps.clone(),
            readme: row.readme.clone(),
            commits: row.commits.clone(),
            last_commit_at: row.last_commit_at,
        }
    }
}

/// 查询分词：按空白拆分 + 小写化。
pub fn terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

/// 单文档评分。所有词都必须命中至少一个字段（AND 语义），否则返回 None。
/// 字段权重：名称 10（前缀命中 13）、标签 8、备注 6、AI 描述 6、语言 4、
/// 依赖 3、README 2、commit 记录 2。子串匹配，对中文友好。
pub fn score_doc(doc: &SearchDoc, terms: &[String]) -> Option<f64> {
    if terms.is_empty() {
        return Some(0.0);
    }
    let mut total = 0.0;
    for term in terms {
        let mut best = 0.0f64;
        let name_lower = doc.name.to_lowercase();
        if name_lower.contains(term) {
            best = best.max(10.0);
            if name_lower.starts_with(term) {
                best = best.max(13.0);
            }
        }
        if doc.tags.iter().any(|t| contains_ci(t, term)) {
            best = best.max(8.0);
        }
        if doc
            .notes
            .as_ref()
            .map(|n| contains_ci(n, term))
            .unwrap_or(false)
        {
            best = best.max(6.0);
        }
        if doc
            .description
            .as_ref()
            .map(|d| contains_ci(d, term))
            .unwrap_or(false)
        {
            best = best.max(6.0);
        }
        if contains_ci(&doc.language, term) {
            best = best.max(4.0);
        }
        if doc.deps.iter().any(|d| contains_ci(d, term)) {
            best = best.max(3.0);
        }
        if doc
            .readme
            .as_ref()
            .map(|r| contains_ci(r, term))
            .unwrap_or(false)
        {
            best = best.max(2.0);
        }
        if doc.commits.iter().any(|c| contains_ci(c, term)) {
            best = best.max(2.0);
        }
        if best == 0.0 {
            return None;
        }
        total += best;
    }
    Some(total)
}

/// 排序：分数降序 → 最近活跃降序 → id 升序（稳定）。
/// 空查询返回全部文档（按活跃度排序），分数为 0。
pub fn rank(docs: &[SearchDoc], query: &str) -> Vec<(i64, f64)> {
    let ts = terms(query);
    let mut scored: Vec<(i64, f64, Option<i64>)> = docs
        .iter()
        .filter_map(|d| score_doc(d, &ts).map(|s| (d.id, s, d.last_commit_at)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.2.cmp(&a.2))
            .then(a.0.cmp(&b.0))
    });
    scored.into_iter().map(|(id, s, _)| (id, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: i64, name: &str, mut f: impl FnMut(&mut SearchDoc)) -> SearchDoc {
        let mut d = SearchDoc {
            id,
            name: name.into(),
            language: "Node.js".into(),
            tags: vec![],
            description: None,
            notes: None,
            deps: vec![],
            readme: None,
            commits: vec![],
            last_commit_at: None,
        };
        f(&mut d);
        d
    }

    #[test]
    fn chinese_term_matches_tags_and_description() {
        let d = doc(1, "spider-demo", |d| {
            d.tags = vec!["爬虫".into()];
            d.description = Some("抓取招聘网站的简历".into());
        });
        assert!(score_doc(&d, &terms("爬虫")).is_some());
        assert!(score_doc(&d, &terms("简历")).is_some());
        assert!(score_doc(&d, &terms("excel")).is_none());
    }

    #[test]
    fn and_semantics_requires_all_terms() {
        let d = doc(1, "resume-spider", |d| {
            d.tags = vec!["爬虫".into()];
        });
        assert!(score_doc(&d, &terms("resume spider")).is_some());
        assert!(score_doc(&d, &terms("resume excel")).is_none());
    }

    #[test]
    fn field_weights_order_name_tags_description() {
        let by_name = doc(1, "spider", |_| {});
        let by_tag = doc(2, "other", |d| d.tags = vec!["spider".into()]);
        let by_desc = doc(3, "other2", |d| d.description = Some("it is a spider".into()));
        let ts = terms("spider");
        let s_name = score_doc(&by_name, &ts).unwrap();
        let s_tag = score_doc(&by_tag, &ts).unwrap();
        let s_desc = score_doc(&by_desc, &ts).unwrap();
        assert!(s_name > s_tag && s_tag > s_desc);
    }

    #[test]
    fn name_prefix_beats_name_substring() {
        let prefix = doc(1, "spider-app", |_| {});
        let substring = doc(2, "my-spider", |_| {});
        let ts = terms("spider");
        assert!(score_doc(&prefix, &ts).unwrap() > score_doc(&substring, &ts).unwrap());
    }

    #[test]
    fn deps_and_commits_are_searchable() {
        let d = doc(1, "etl", |d| {
            d.deps = vec!["xlsx-populate".into()];
            d.commits = vec!["fix excel export".into()];
        });
        assert!(score_doc(&d, &terms("xlsx")).is_some());
        assert!(score_doc(&d, &terms("excel")).is_some());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let d = doc(1, "Resume-Parser", |_| {});
        assert!(score_doc(&d, &terms("RESUME")).is_some());
    }

    #[test]
    fn rank_orders_by_score_then_activity() {
        let high = doc(1, "spider-a", |d| d.last_commit_at = Some(100));
        let high_old = doc(2, "spider-b", |d| d.last_commit_at = Some(50));
        let low = doc(3, "other", |d| {
            d.tags = vec!["spider".into()];
            d.last_commit_at = Some(999);
        });
        let ranked = rank(&[low, high_old, high], "spider");
        assert_eq!(ranked, vec![(1, ranked[0].1), (2, ranked[1].1), (3, ranked[2].1)]);
        // 前两个都是名称前缀命中，分数相同，靠活跃度分先后
        assert_eq!(ranked[0].1, ranked[1].1);
        assert!(ranked[1].1 > ranked[2].1);
    }

    #[test]
    fn empty_query_returns_all_by_activity() {
        let a = doc(1, "a", |d| d.last_commit_at = Some(10));
        let b = doc(2, "b", |d| d.last_commit_at = Some(20));
        let ranked = rank(&[a, b], "   ");
        assert_eq!(ranked, vec![(2, 0.0), (1, 0.0)]);
    }

    #[test]
    fn from_row_maps_all_fields() {
        let row = ProjectRow {
            id: 7,
            path: r"C:\x".into(),
            name: "x".into(),
            language: "Rust".into(),
            status: "ok".into(),
            description: Some("d".into()),
            tags: vec!["t".into()],
            notes: Some("n".into()),
            deps: vec!["serde".into()],
            commits: vec!["c".into()],
            readme: Some("r".into()),
            git_remote: None,
            structure: vec![],
            last_commit_at: Some(5),
            analyzed_at: None,
            ai_source: None,
            updated_at: 1,
        };
        let d = SearchDoc::from_row(&row);
        assert_eq!(d.id, 7);
        assert_eq!(d.tags, vec!["t"]);
        assert_eq!(d.last_commit_at, Some(5));
    }
}
