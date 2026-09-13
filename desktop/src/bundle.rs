//! Desktop 的 bundle 边界（P4R Batch 6A §6/§9/§10）。
//!
//! ## 契约
//!
//! Desktop **不是**一个能独立运行的可执行文件：它必须与随包 sidecar、前端产物与
//! 数据集一同发布。因此「Desktop 能启动」与「发布包完整」是两件事，运行时必须能
//! 区分它们，否则缺失资源会表现为一个空白窗口。
//!
//! 解析规则（刻意严格，无任何开发目录回退）：
//!
//! ```text
//! bundle root = 当前可执行文件所在目录
//! sidecar     = <root>/nav-core-cli.exe  或  <root>/nav-core-cli-<triple>.exe
//! web root    = <root>/web
//! dataset     = <root>/data/europe-v5
//! manifest    = <root>/bundle-manifest.json
//! ```
//!
//! 之所以刻意不提供「回退到 nav-core/target/debug/nav-core-cli.exe」这类便利路径：
//! 那会让「打包正确」与「开发机上恰好有编译产物」在观测上不可区分。P4R Batch 6A
//! §20 的 M3 mutation 正是把这个回退当成缺陷注入——没有回退，mutation 才必然失败。

use std::path::{Path, PathBuf};

use crate::sha256;

/// bundle 清单文件名。
pub const MANIFEST_NAME: &str = "bundle-manifest.json";
/// sidecar 的基准名（不带扩展名与可选 target triple 后缀）。
pub const SIDECAR_BASE: &str = "nav-core-cli";
/// 随包前端目录名。
pub const WEB_DIR_NAME: &str = "web";
/// 随包数据集相对路径。
pub const DATASET_REL: &str = "data/europe-v5";
/// 数据集在运行时**必需**的文件（缺失即视为数据集不完整）。
pub const DATASET_REQUIRED_FILES: &[&str] = &[
    "manifest.json",
    "routing.graph",
    "junction.graph",
    "search.db",
];

/// bundle 根目录：可执行文件所在目录。无法取得时返回当前目录（并会被后续存在性检查挡住）。
pub fn bundle_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 解析 sidecar 路径。清单中声明了名字时以清单为准（发布产物的身份由清单定义）。
pub fn resolve_sidecar(root: &Path, manifest: Option<&Manifest>) -> Option<PathBuf> {
    let mut names: Vec<String> = Vec::new();
    if let Some(m) = manifest {
        if let Some(n) = m.sidecar_name() {
            names.push(n.to_string());
        }
    }
    for n in [
        format!("{SIDECAR_BASE}.exe"),
        format!("{SIDECAR_BASE}-{TRIPLE}.exe"),
    ] {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
        .into_iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
}

/// 本仓库的发布目标三元组（Windows-only 产品）。
pub const TRIPLE: &str = "x86_64-pc-windows-msvc";

/// bundle 清单的结构。
///
/// 用 `serde_json::Value` 逐字段读取而不是 derive：清单由打包脚本生成，字段增删
/// 必须能在**不重新编译**旧 Desktop 的情况下被检出来（缺失即显式报错），
/// 而 derive 会把「字段缺失」变成反序列化失败，错误信息丢失具体缺了哪一项。
#[derive(Debug, Clone)]
pub struct Manifest {
    pub raw: serde_json::Value,
}

impl Manifest {
    pub fn load(root: &Path) -> Result<Option<Manifest>, String> {
        let path = root.join(MANIFEST_NAME);
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("读取 {MANIFEST_NAME} 失败: {e}"))?;
        let raw: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("{MANIFEST_NAME} 不是合法 JSON: {e}"))?;
        let schema = raw.get("schema").and_then(|v| v.as_u64()).unwrap_or(0);
        if schema != 1 {
            return Err(format!(
                "{MANIFEST_NAME} 的 schema={schema}，本版本 Desktop 只理解 schema=1"
            ));
        }
        Ok(Some(Manifest { raw }))
    }

    fn str_at(&self, path: &[&str]) -> Option<&str> {
        let mut cur = &self.raw;
        for k in path {
            cur = cur.get(*k)?;
        }
        cur.as_str()
    }

    fn u64_at(&self, path: &[&str]) -> Option<u64> {
        let mut cur = &self.raw;
        for k in path {
            cur = cur.get(*k)?;
        }
        cur.as_u64()
    }

    pub fn sidecar_name(&self) -> Option<&str> {
        self.str_at(&["sidecar", "name"])
    }
    pub fn sidecar_sha256(&self) -> Option<&str> {
        self.str_at(&["sidecar", "sha256"])
    }
    pub fn sidecar_bytes(&self) -> Option<u64> {
        self.u64_at(&["sidecar", "bytes"])
    }
    pub fn source_commit(&self) -> Option<&str> {
        self.str_at(&["source", "commit"])
    }
    pub fn dataset_tree_sha256(&self) -> Option<&str> {
        self.str_at(&["dataset", "tree_sha256"])
    }
    pub fn dataset_files(&self) -> Option<u64> {
        self.u64_at(&["dataset", "files"])
    }
    pub fn dataset_bytes(&self) -> Option<u64> {
        self.u64_at(&["dataset", "bytes"])
    }
    pub fn dataset_content_fingerprint(&self) -> Option<&str> {
        self.str_at(&["dataset", "content_fingerprint"])
    }
    pub fn basemap_present(&self) -> bool {
        self.raw
            .get("basemap")
            .and_then(|b| b.get("present"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    pub fn fonts_present(&self) -> bool {
        self.raw
            .get("fonts")
            .and_then(|b| b.get("present"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

/// sidecar 的身份判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarIdentity {
    /// 与清单逐字节一致。
    Verified,
    /// 清单缺失：release 下是打包缺陷（致命），debug 下允许但显式标注。
    ManifestMissing,
    /// 与清单不符：任何 profile 下都是致命。
    Mismatch { expected: String, actual: String },
}

impl SidecarIdentity {
    pub fn label(&self) -> &'static str {
        match self {
            SidecarIdentity::Verified => "verified",
            SidecarIdentity::ManifestMissing => "unverified-no-manifest",
            SidecarIdentity::Mismatch { .. } => "mismatch",
        }
    }
}

/// 校验 sidecar 的字节身份。
///
/// 返回 `(identity, sha256, bytes)`。文件不存在由调用方在此之前处理（那是「缺失」，
/// 与「存在但身份不符」是两类缺陷，处置不同）。
pub fn verify_sidecar(
    path: &Path,
    manifest: Option<&Manifest>,
) -> Result<(SidecarIdentity, String, u64), String> {
    let (actual, bytes) =
        sha256::file_hex(path).map_err(|e| format!("摘要 {} 失败: {e}", path.display()))?;
    let Some(m) = manifest else {
        return Ok((SidecarIdentity::ManifestMissing, actual, bytes));
    };
    let Some(expected) = m.sidecar_sha256() else {
        return Err(format!("{MANIFEST_NAME} 未声明 sidecar.sha256"));
    };
    if !actual.eq_ignore_ascii_case(expected) {
        return Ok((
            SidecarIdentity::Mismatch {
                expected: expected.to_string(),
                actual: actual.clone(),
            },
            actual,
            bytes,
        ));
    }
    if let Some(declared) = m.sidecar_bytes() {
        if declared != bytes {
            return Ok((
                SidecarIdentity::Mismatch {
                    expected: format!("{declared} B（清单声明的字节数）"),
                    actual: format!("{bytes} B"),
                },
                actual,
                bytes,
            ));
        }
    }
    Ok((SidecarIdentity::Verified, actual, bytes))
}

/// 数据集的结构身份：文件数、总字节、必需文件是否齐备。
///
/// 刻意**不在每次启动时重算 356 MB 的树摘要**：启动延迟是用户直接感知的量，
/// 而完整摘要由打包校验（`verify-bundle.ps1`）承担——那里本来就是「发布前一次性
/// 验完」的场合。此处只做能廉价发现「数据集没随包发出」「被换成了另一个数据集」
/// 的检查，并如实标注该边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetCheck {
    pub files: u64,
    pub bytes: u64,
    pub missing_required: Vec<String>,
}

impl DatasetCheck {
    pub fn problems(&self, manifest: Option<&Manifest>) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.missing_required {
            out.push(format!("缺少必需文件 {f}"));
        }
        if let Some(m) = manifest {
            if let Some(n) = m.dataset_files() {
                if n != self.files {
                    out.push(format!("文件数不符：清单 {n}，实际 {}", self.files));
                }
            }
            if let Some(b) = m.dataset_bytes() {
                if b != self.bytes {
                    out.push(format!("总字节不符：清单 {b}，实际 {}", self.bytes));
                }
            }
        }
        out
    }
}

/// 扫描数据集目录（不读内容，只统计与存在性）。
pub fn inspect_dataset(dir: &Path) -> Result<DatasetCheck, String> {
    if !dir.is_dir() {
        return Err(format!("数据集目录不存在: {}", dir.display()));
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| format!("读取 {} 失败: {e}", d.display()))?;
        for entry in rd {
            let entry = entry.map_err(|e| format!("遍历 {} 失败: {e}", d.display()))?;
            let p = entry.path();
            let md = entry
                .metadata()
                .map_err(|e| format!("读取 {} 元数据失败: {e}", p.display()))?;
            if md.is_dir() {
                stack.push(p);
            } else {
                files += 1;
                bytes += md.len();
            }
        }
    }
    let missing_required = DATASET_REQUIRED_FILES
        .iter()
        .filter(|f| !dir.join(f).is_file())
        .map(|f| (*f).to_string())
        .collect();
    Ok(DatasetCheck {
        files,
        bytes,
        missing_required,
    })
}

/// 单个文件的 (相对路径, 字节数, 摘要) 列表的规范化树摘要。
///
/// 与打包脚本同一算法：按相对路径（正斜杠、区分大小写）排序，每行
/// `relpath\0size\0sha256\n`，对整串再做一次 SHA-256。对文件系统遍历顺序不敏感。
pub fn tree_digest_lines(entries: &mut [(String, u64, String)]) -> String {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = sha256::Sha256::new();
    for (rel, size, sha) in entries.iter() {
        h.update(rel.as_bytes());
        h.update(&[0u8]);
        h.update(size.to_string().as_bytes());
        h.update(&[0u8]);
        h.update(sha.as_bytes());
        h.update(b"\n");
    }
    sha256::hex(&h.finish())
}

/// 深度校验：重算整棵数据集目录的树摘要。
///
/// 默认**不**执行——373 MB 的全量摘要会把启动延迟从毫秒级拉到秒级，而这是用户
/// 直接感知的量。它以 `--verify-dataset-digest` 显式开启，供发布校验与缺陷注入
/// 验证使用：此时「数据集被改动」在**启动路径**上就会失败，而不只是打包脚本发现。
pub fn dataset_tree_digest(dir: &Path) -> Result<(String, u64, u64), String> {
    let mut entries: Vec<(String, u64, String)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut total_bytes = 0u64;
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| format!("读取 {} 失败: {e}", d.display()))?;
        for entry in rd {
            let entry = entry.map_err(|e| format!("遍历 {} 失败: {e}", d.display()))?;
            let p = entry.path();
            let md = entry
                .metadata()
                .map_err(|e| format!("读取 {} 元数据失败: {e}", p.display()))?;
            if md.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p
                .strip_prefix(dir)
                .map_err(|e| format!("相对路径计算失败 {}: {e}", p.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let (sha, bytes) =
                sha256::file_hex(&p).map_err(|e| format!("摘要 {} 失败: {e}", p.display()))?;
            total_bytes += bytes;
            entries.push((rel, bytes, sha));
        }
    }
    let n = entries.len() as u64;
    Ok((tree_digest_lines(&mut entries), n, total_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ets2nav-bundle-test-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn manifest_missing_is_not_an_error_but_a_status() {
        let root = tmpdir("no-manifest");
        let m = Manifest::load(&root).unwrap();
        assert!(m.is_none(), "清单缺失必须表达为 None 而不是解析失败");
    }

    #[test]
    fn manifest_schema_is_enforced() {
        let root = tmpdir("schema");
        std::fs::write(root.join(MANIFEST_NAME), r#"{"schema":2}"#).unwrap();
        let err = Manifest::load(&root).unwrap_err();
        assert!(err.contains("schema=2"), "{err}");
        std::fs::write(
            root.join(MANIFEST_NAME),
            r#"{"schema":1,"sidecar":{"sha256":"ab"}}"#,
        )
        .unwrap();
        let m = Manifest::load(&root).unwrap().unwrap();
        assert_eq!(m.sidecar_sha256(), Some("ab"));
        assert_eq!(m.sidecar_name(), None);
        assert!(!m.basemap_present());
    }

    #[test]
    fn sidecar_identity_distinguishes_mismatch_from_missing() {
        let root = tmpdir("identity");
        let sidecar = root.join("nav-core-cli.exe");
        std::fs::write(&sidecar, b"payload").unwrap();
        let (sha, bytes) = sha256::file_hex(&sidecar).unwrap();
        assert_eq!(bytes, 7);

        // 无清单 → ManifestMissing（不是 Mismatch）
        let (id, actual, _) = verify_sidecar(&sidecar, None).unwrap();
        assert_eq!(id, SidecarIdentity::ManifestMissing);
        assert_eq!(actual, sha);

        // 清单一致 → Verified
        let text = format!(r#"{{"schema":1,"sidecar":{{"sha256":"{sha}","bytes":7}}}}"#);
        std::fs::write(root.join(MANIFEST_NAME), text).unwrap();
        let m = Manifest::load(&root).unwrap();
        let (id, _, _) = verify_sidecar(&sidecar, m.as_ref()).unwrap();
        assert_eq!(id, SidecarIdentity::Verified);

        // 内容改变 → Mismatch，且带上两个摘要（诊断需要能看出「换了哪个文件」）
        std::fs::write(&sidecar, b"payload2").unwrap();
        let (id, actual, _) = verify_sidecar(&sidecar, m.as_ref()).unwrap();
        match id {
            SidecarIdentity::Mismatch {
                expected,
                actual: a,
            } => {
                assert_eq!(expected, sha);
                assert_eq!(a, actual);
                assert_ne!(a, sha);
            }
            other => panic!("期望 Mismatch，实际 {other:?}"),
        }
    }

    #[test]
    fn dataset_inspection_counts_and_required_files() {
        let dir = tmpdir("dataset");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("manifest.json"), b"{}").unwrap();
        std::fs::write(dir.join("routing.graph"), b"12345").unwrap();
        std::fs::write(dir.join("junction.graph"), b"123").unwrap();
        std::fs::write(dir.join("sub/extra.bin"), b"12").unwrap();
        let c = inspect_dataset(&dir).unwrap();
        assert_eq!(c.files, 4);
        assert_eq!(c.bytes, 12);
        assert_eq!(c.missing_required, vec!["search.db".to_string()]);
        let problems = c.problems(None);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("search.db"));
    }

    #[test]
    fn dataset_missing_dir_is_an_error() {
        let p = std::env::temp_dir().join("ets2nav-bundle-test-absent-dataset");
        let _ = std::fs::remove_dir_all(&p);
        assert!(inspect_dataset(&p).is_err());
    }

    #[test]
    fn tree_digest_is_order_insensitive_but_content_sensitive() {
        let mk = || {
            vec![
                ("b.txt".to_string(), 2u64, "bb".to_string()),
                ("a/x.bin".to_string(), 1u64, "aa".to_string()),
            ]
        };
        let mut v1 = mk();
        let d1 = tree_digest_lines(&mut v1);
        let mut v2 = mk();
        v2.reverse();
        assert_eq!(d1, tree_digest_lines(&mut v2), "遍历顺序不得影响树摘要");

        let mut v3 = mk();
        v3[0].2 = "cc".to_string();
        assert_ne!(d1, tree_digest_lines(&mut v3), "内容改变必须改变树摘要");
        let mut v4 = mk();
        v4[0].1 = 3;
        assert_ne!(d1, tree_digest_lines(&mut v4), "字节数改变必须改变树摘要");
    }

    #[test]
    fn resolve_sidecar_prefers_manifest_name_and_never_guesses_outside_root() {
        let root = tmpdir("resolve");
        assert!(
            resolve_sidecar(&root, None).is_none(),
            "空目录不得解析出路径"
        );
        std::fs::write(root.join("nav-core-cli-x86_64-pc-windows-msvc.exe"), b"x").unwrap();
        let got = resolve_sidecar(&root, None).expect("应命中 triple 后缀名");
        assert_eq!(
            got.file_name().unwrap(),
            "nav-core-cli-x86_64-pc-windows-msvc.exe"
        );
        // 解析结果必须始终位于 bundle root 之内
        assert!(got.starts_with(&root));

        // 清单声明的名字优先
        std::fs::write(root.join("custom.exe"), b"y").unwrap();
        let text = r#"{"schema":1,"sidecar":{"name":"custom.exe","sha256":"00"}}"#;
        std::fs::write(root.join(MANIFEST_NAME), text).unwrap();
        let m = Manifest::load(&root).unwrap();
        let got = resolve_sidecar(&root, m.as_ref()).unwrap();
        assert_eq!(got.file_name().unwrap(), "custom.exe");
    }
}
