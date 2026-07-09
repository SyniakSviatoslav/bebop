//! Bebop core — the self-contained deterministic guard kernel, compiled to WASM.
//!
//! This IS the "kernel" the operator wanted inside the bebop repo: the trust boundary that no
//! cloned `bebop.json` can relax. Rust, no clock/RNG/network in the decision path. The CLI calls
//! `decide` via the wasm boundary; if the wasm is absent it falls back to the TS port (parity).
//!
//! NOTE: this is bebop's *own* guard kernel (red-line + scope deny/pass + decision log) — NOT the
//! dowiz food-delivery order state machine that shares the name in the larger monorepo.

use std::sync::OnceLock;

// ── glob → regex (faithful port of guard.ts `toRegExp`) ────────────────────────────────────────
fn glob_to_regex(glob: &str) -> String {
    let mut re = String::new();
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' {
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                re.push_str(".*");
                i += 1; // consume second '*'
                if i + 1 < chars.len() && chars[i + 1] == '/' {
                    i += 1; // consume the slash after "**/"
                }
            } else {
                re.push_str("[^/]*");
            }
        } else if c == '?' {
            re.push_str("[^/]");
        } else if ".+^${}()|[]\\".contains(c) {
            re.push('\\');
            re.push(c);
        } else {
            re.push(c);
        }
        i += 1;
    }
    format!("^(?:{re})$")
}

mod glob {
    use std::collections::HashMap;
    use std::sync::OnceLock;

    // caches compiled regexes (Rust's regex::Regex isn't used to keep the wasm tiny + no regex crate
    // dependency — we use a tiny matcher instead, see matches()).
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();

    fn cache() -> &'static std::sync::Mutex<HashMap<String, String>> {
        CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
    }

    /// Glob match faithful to guard.ts's `new RegExp(...).test(p)`:
    /// `**` = any chars (cross-segment), `*` = within one segment, `?` = one char.
    pub fn matches(glob: &str, p: &str) -> bool {
        let mut c = cache().lock().unwrap();
        let regex = c.entry(glob.to_string()).or_insert_with(|| super::glob_to_regex(glob));
        let re = regex.clone();
        drop(c);
        super::regex_test(&re, p)
    }
}

/// Minimal regex tester supporting the subset we generate: `.*`, `[^/]*`, `[^/]`, escaped literals,
/// `^`/`$` anchors, and literal chars. No backtracking needed for these patterns.
fn regex_test(re: &str, p: &str) -> bool {
    // We only generate anchored patterns `^(?:...)$`; strip anchors for the match loop.
    let inner = re.trim_start_matches("^(?:").trim_end_matches(")$");
    backtrack(inner, p, 0, 0)
}

fn backtrack(pat: &str, text: &str, pi: usize, ti: usize) -> bool {
    let pchars: Vec<char> = pat.chars().collect();
    let tchars: Vec<char> = text.chars().collect();
    let plen = pchars.len();
    let tlen = tchars.len();
    let mut p = pi;
    let mut t = ti;
    while p < plen {
        let c = pchars[p];
        if c == '.' && p + 1 < plen && pchars[p + 1] == '*' {
            // .* — greedy match any chars
            p += 2;
            for skip in t..=tlen {
                if backtrack(pat, text, p, skip) {
                    return true;
                }
            }
            return false;
        } else if c == '[' && p + 1 < plen && pchars[p + 1] == '^' {
            // [^/] or [^...] — one char not in the set; with a trailing '*' it becomes 0+.
            let close = (p + 2..plen).find(|&i| pchars[i] == ']').unwrap_or(plen);
            let star = close + 1 < plen && pchars[close + 1] == '*';
            let set: String = pchars[p + 2..close].iter().collect();
            if t >= tlen && !star {
                return false;
            }
            if set == "/" {
                if star {
                    // 0+ non-slash chars (greedy, then backtrack)
                    let mut consumed = 0;
                    while t + consumed < tlen && tchars[t + consumed] != '/' {
                        consumed += 1;
                    }
                    for k in (0..=consumed).rev() {
                        if backtrack(pat, text, close + 2, t + k) {
                            return true;
                        }
                    }
                    return false;
                } else if tchars[t] == '/' {
                    return false;
                }
            } else if !star {
                // only the single-char negation sets we emit here
                if set.chars().any(|s| s == tchars[t]) {
                    return false;
                }
            } else {
                // 0+ chars not in set (greedy, then backtrack)
                let mut consumed = 0;
                while t + consumed < tlen && !set.chars().any(|s| s == tchars[t + consumed]) {
                    consumed += 1;
                }
                for k in (0..=consumed).rev() {
                    if backtrack(pat, text, close + 2, t + k) {
                        return true;
                    }
                }
                return false;
            }
            p = close + 1 + if star { 1 } else { 0 };
            t += 1;
        } else if c == '\\' && p + 1 < plen {
            // escaped literal
            p += 1;
            if t >= tlen || pchars[p] != tchars[t] {
                return false;
            }
            p += 1;
            t += 1;
        } else {
            if t >= tlen || pchars[p] != tchars[t] {
                return false;
            }
            p += 1;
            t += 1;
        }
    }
    t == tlen
}

// ── red-line + scope globs (mirror of guard.ts) ────────────────────────────────────────────────
pub fn red_line_globs() -> Vec<String> {
    vec![
        "**/auth/**",
        "**/migrations/**",
        "**/rls/**",
        "**/*.sql",
        "**/packages/db/migrations/**",
        "**/money/**",
        "**/payments/**",
        "**/bulk-edit/**",
        "**/secret/**",
        "**/secrets/**",
        "**/.env",
        "**/.env.*",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub fn default_scope_globs() -> Vec<String> {
    vec!["tools/bebop/**", "docs/design/dowiz-agent-cli/**"]
        .into_iter()
        .map(String::from)
        .collect()
}

// ── decide ─────────────────────────────────────────────────────────────────────────────────────
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Decision {
    pub ok: bool,
    pub kind: String, // "redline" | "scope" | "ok"
    pub reason: String,
    pub deny: bool,
}

/// Decide on a target path + operation. `op` = "read" | "edit" | "run" | "dispatch" | "recall".
/// `extra_deny` strengthens red-lines (user settings only). `scope` narrows allowed surface.
pub fn decide(
    target: &str,
    op: &str,
    extra_deny: &[String],
    scope: &[String],
    cwd: &str,
) -> Decision {
    // 1. red-line deny (hardcoded core — cannot be relaxed)
    for g in red_line_globs() {
        if glob::matches(&g, target) {
            return Decision {
                ok: false,
                kind: "redline".into(),
                reason: "red-line: requires explicit human go-ahead (auth/money/RLS/migrations/secrets).".into(),
                deny: true,
            };
        }
    }
    // 2. user-supplied deny globs (strengthen only)
    for g in extra_deny {
        if glob::matches(g, target) {
            return Decision {
                ok: false,
                kind: "redline".into(),
                reason: "red-line (user deny): blocked by ~/.bebop/settings.json.".into(),
                deny: true,
            };
        }
    }
    // 3. scope check (only for file-bearing ops)
    if matches!(op, "read" | "edit" | "run" | "dispatch") && !scope.is_empty() {
        let rel = if target.starts_with('/') {
            strip_prefix(cwd, target)
        } else {
            target.to_string()
        };
        let candidates = [target, rel.as_str()];
        let allowed = scope.iter().any(|g| candidates.iter().any(|c| glob::matches(g, c)));
        if !allowed {
            return Decision {
                ok: false,
                kind: "scope".into(),
                reason: "scope: outside the agreed surface; re-ask before touching.".into(),
                deny: true,
            };
        }
    }
    Decision {
        ok: true,
        kind: "ok".into(),
        reason: String::new(),
        deny: false,
    }
}

fn strip_prefix(cwd: &str, abs: &str) -> String {
    if let Some(suffix) = abs.strip_prefix(cwd) {
        suffix.trim_start_matches('/').to_string()
    } else {
        abs.to_string()
    }
}

// ── append-only decision log (the immutable kernel memory of what it refused) ──────────────────
use std::sync::Mutex;

pub struct Kernel {
    log: Mutex<Vec<String>>,
}

static KERNEL: OnceLock<Kernel> = OnceLock::new();

fn kernel() -> &'static Kernel {
    KERNEL.get_or_init(|| Kernel { log: Mutex::new(Vec::new()) })
}

/// Record a decision line (JSON) into the append-only log. Returns the seq number.
pub fn record(target: &str, op: &str, decision: &Decision) -> u64 {
    let k = kernel();
    let mut log = k.log.lock().unwrap();
    let seq = log.len() as u64;
    let line = serde_json::json!({
        "seq": seq,
        "op": op,
        "target": target,
        "ok": decision.ok,
        "kind": decision.kind,
    })
    .to_string();
    log.push(line);
    seq
}

pub fn log_len() -> u64 {
    kernel().log.lock().unwrap().len() as u64
}

// ── retriever (VSA port: deterministic hash embeddings, no network) ────────────────────────────
pub mod retriever {
    use super::*;

    /// FNV-1a 64-bit hash — deterministic, no deps.
    pub fn fnv1a(s: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// Build a fixed-dim binary VSA vector from text (token hashes → bipolar). Deterministic.
    pub fn embed(text: &str, dim: usize) -> Vec<i8> {
        let mut v = vec![0i8; dim];
        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.is_empty() {
            return v;
        }
        for (i, tok) in tokens.iter().cycle().take(dim).enumerate() {
            let h = fnv1a(tok);
            v[i] = if (h & 1) == 1 { 1 } else { -1 };
        }
        v
    }

    /// Cosine-over-bipolar similarity in [-1, 1].
    pub fn similarity(a: &[i8], b: &[i8]) -> f64 {
        if a.is_empty() || b.is_empty() || a.len() != b.len() {
            return 0.0;
        }
        let dot: i32 = a.iter().zip(b).map(|(x, y)| (*x as i32) * (*y as i32)).sum();
        dot as f64 / a.len() as f64
    }

    pub fn estimate_tokens(text: &str) -> usize {
        // ~4 chars/token heuristic; deterministic.
        (text.chars().count() + 3) / 4
    }

    // ── wasm C-ABI surface (hand-rolled, no wasm-bindgen → tiny, zero host deps) ────────────────
    // Pattern: calls that return strings write into a global buffer; the host reads it via
    // bebop_result_ptr()/bebop_result_len() then copies it out. No malloc coordination needed.
    // Mutex-wrapped so concurrent wasm calls (single-threaded, but sound) can't create UB.
    static RESULT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

    pub fn set_result(s: String) {
        *RESULT.lock().unwrap() = s;
    }

    #[no_mangle]
    pub extern "C" fn bebop_result_ptr() -> *const u8 {
        RESULT.lock().unwrap().as_ptr()
    }

    #[no_mangle]
    pub extern "C" fn bebop_result_len() -> usize {
        RESULT.lock().unwrap().len()
    }

    /// Decide from JSON args. `args` = {"target","op","extra_deny":[..],"scope":[..],"cwd"}.
    /// Result written to the shared buffer as JSON Decision.
    #[no_mangle]
    pub extern "C" fn bebop_decide(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => {
                set_result(format!("{{\"ok\":false,\"kind\":\"error\",\"reason\":\"{e}\",\"deny\":true}}"));
                return;
            }
        };
        let target = v["target"].as_str().unwrap_or("");
        let op = v["op"].as_str().unwrap_or("edit");
        let extra: Vec<String> = v["extra_deny"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let scope: Vec<String> = v["scope"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let cwd = v["cwd"].as_str().unwrap_or("");
        let d = decide(target, op, &extra, &scope, cwd);
        record(target, op, &d);
        set_result(serde_json::to_string(&d).unwrap_or_else(|_| "{\"ok\":false}".into()));
    }

    /// Embed text → JSON array of i8. `args` = {"text","dim"}.
    #[no_mangle]
    pub extern "C" fn bebop_embed(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap_or(serde_json::json!({}));
        let text = v["text"].as_str().unwrap_or("");
        let dim = v["dim"].as_u64().unwrap_or(256) as usize;
        let emb = embed(text, dim);
        set_result(serde_json::to_string(&emb).unwrap_or_else(|_| "[]".into()));
    }

    /// Similarity of two JSON i8 arrays. `args` = {"a":[..],"b":[..]}.
    #[no_mangle]
    pub extern "C" fn bebop_similarity(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap_or(serde_json::json!({}));
        let to_vec = |key: &str| -> Vec<i8> {
            v[key]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_i64().map(|n| n as i8)).collect())
                .unwrap_or_default()
        };
        let s = similarity(&to_vec("a"), &to_vec("b"));
        set_result(format!("{s}"));
    }

    /// Estimate tokens for text. Returns the count as a JSON number string.
    #[no_mangle]
    pub extern "C" fn bebop_estimate_tokens(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap_or(serde_json::json!({}));
        let n = estimate_tokens(v["text"].as_str().unwrap_or(""));
        set_result(format!("{n}"));
    }

    /// Append-only log length.
    #[no_mangle]
    pub extern "C" fn bebop_log_len() -> u64 {
        log_len()
    }

    /// Export the decision log as a JSON array string.
    #[no_mangle]
    pub extern "C" fn bebop_export_log() {
        let k = kernel();
        let log = k.log.lock().unwrap();
        set_result(serde_json::to_string(&*log).unwrap_or_else(|_| "[]".into()));
    }
}

// ── math: deterministic linear algebra + Kalman (faithful, zero-dep port of matrix.ts) ─────────
// These are the deterministic cores that the TS analytics layer uses (arch-mine SVD coupling,
// governor PCA anomaly, kalman rate-smoothing). Porting them to Rust/WASM lets the sovereign node
// run the SAME bit-faithful math with no JS runtime, no RNG/Date, no network. Kept dependency-free
// (Jacobi EVD, same algorithm as matrix.ts) so the wasm stays tiny and the signatures match.
pub mod math {
    use serde_json::Value;

    pub type Mat = Vec<Vec<f64>>;
    pub type Vecf = Vec<f64>;

    fn transpose(a: &Mat) -> Mat {
        let n = a.len();
        let m = a[0].len();
        let mut t = vec![vec![0.0; n]; m];
        for i in 0..n { for j in 0..m { t[j][i] = a[i][j]; } }
        t
    }
    fn matmul(a: &Mat, b: &Mat) -> Mat {
        let n = a.len(); let k = b.len(); let m = b[0].len();
        let mut out = vec![vec![0.0; m]; n];
        for i in 0..n { for p in 0..k { let aip = a[i][p]; if aip == 0.0 { continue; } for j in 0..m { out[i][j] += aip * b[p][j]; } } }
        out
    }

    /// Jacobi eigenvalue decomposition of a symmetric matrix (faithful port of matrix.ts jacobiEVD).
    fn jacobi_evd(a_in: &Mat, sweeps: usize) -> (Vecf, Mat) {
        let n = a_in.len();
        let mut a = a_in.clone();
        let mut v = vec![vec![0.0; n]; n];
        for i in 0..n { v[i][i] = 1.0; }
        for _ in 0..sweeps {
            let mut off = 0.0;
            for p in 0..n { for q in (p+1)..n { off += a[p][q] * a[p][q]; } }
            if off < 1e-18 { break; }
            for p in 0..n { for q in (p+1)..n {
                let apq = a[p][q];
                if apq.abs() < 1e-300 { continue; }
                let app = a[p][p]; let aqq = a[q][q];
                let phi = (aqq - app) / (2.0 * apq);
                let t = phi.signum() / (phi.abs() + (phi*phi + 1.0).sqrt());
                let c = 1.0 / (t*t + 1.0).sqrt();
                let s = t * c;
                // rotate all rows/cols EXCEPT the p,q block itself
                for i in 0..n {
                    if i == p || i == q { continue; }
                    let aip = a[i][p]; let aiq = a[i][q];
                    a[i][p] = c*aip - s*aiq; a[i][q] = s*aip + c*aiq;
                    let api = a[p][i]; let aqi = a[q][i];
                    a[p][i] = c*api - s*aqi; a[q][i] = s*api + c*aqi;
                    let vip = v[i][p]; let viq = v[i][q];
                    v[i][p] = c*vip - s*viq; v[i][q] = s*vip + c*viq;
                }
                // atomic 2x2 block update (uses pre-rotation app/aqq/apq)
                let app_new = c*c*app - 2.0*s*c*apq + s*s*aqq;
                let aqq_new = s*s*app + 2.0*s*c*apq + c*c*aqq;
                a[p][p] = app_new; a[q][q] = aqq_new; a[p][q] = 0.0; a[q][p] = 0.0;
            } }
        }
        let values: Vecf = (0..n).map(|i| a[i][i]).collect();
        (values, v)
    }

    /// Two-sided SVD via symmetric EVD of AᵀA / AAᵀ (faithful port of matrix.ts svd).
    pub fn svd(a_in: &Mat) -> (Mat, Vecf, Mat) {
        let m = a_in.len(); let n = a_in[0].len();
        let (u, svals, v) = if m >= n {
            let ata = matmul(&transpose(a_in), a_in);
            let (ev, vecs) = jacobi_evd(&ata, 32);
            let s: Vecf = ev.iter().map(|l| (l.max(0.0)).sqrt()).collect();
            let av = matmul(a_in, &vecs);
            let u = av.iter().map(|row| row.iter().enumerate().map(|(j, x)| if s[j] > 1e-12 { *x / s[j] } else { 0.0 }).collect()).collect();
            (u, s, vecs)
        } else {
            let aat = matmul(a_in, &transpose(a_in));
            let (ev, vecs) = jacobi_evd(&aat, 32);
            let s: Vecf = ev.iter().map(|l| (l.max(0.0)).sqrt()).collect();
            let atu = matmul(&transpose(a_in), &vecs);
            let v = atu.iter().map(|row| row.iter().enumerate().map(|(j, x)| if s[j] > 1e-12 { *x / s[j] } else { 0.0 }).collect()).collect();
            (vecs, s, v)
        };
        // sort descending by singular value
        let mut order: Vec<usize> = (0..svals.len()).collect();
        order.sort_by(|&i, &j| svals[j].partial_cmp(&svals[i]).unwrap_or(std::cmp::Ordering::Equal));
        let s = order.iter().map(|&i| svals[i]).collect();
        let u = u.iter().map(|row| order.iter().map(|&i| row[i]).collect()).collect();
        let v = v.iter().map(|row| order.iter().map(|&i| row[i]).collect()).collect();
        (u, s, v)
    }

    /// 1-D Kalman prediction/update (faithful port of kalman.ts kalman1dStep).
    pub fn kalman_1d(z: f64, x: f64, p: f64, q: f64, r: f64) -> (f64, f64) {
        let x_pred = x; let p_pred = p + q;
        let k = if (p_pred + r) != 0.0 { p_pred / (p_pred + r) } else { 0.0 };
        let x_upd = x_pred + k * (z - x_pred);
        let p_upd = (1.0 - k) * p_pred;
        (x_upd, p_upd)
    }

    // ── wasm C-ABI surface (hand-rolled, consistent with retriever) ──
    // NOTE: the shared result buffer + bebop_result_ptr/len live in `retriever`; we reuse them so
    // there is exactly ONE C-ABI result accessor (the host reads it once).
    /// SVD of a JSON matrix `[[..],[..]]` → {"U":..,"S":..,"V":..}.
    #[no_mangle]
    pub extern "C" fn bebop_svd(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: Value = match serde_json::from_slice(bytes) { Ok(v) => v, Err(e) => { crate::retriever::set_result(format!("{{\"error\":\"{e}\"}}")); return; } };
        let mat: Mat = v.as_array().map(|rows| rows.iter().map(|r| r.as_array().map(|c| c.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default()).collect()).unwrap_or_default();
        if mat.is_empty() { crate::retriever::set_result("{\"U\":[],\"S\":[],\"V\":[]}".into()); return; }
        let (u, s, vv) = svd(&mat);
        crate::retriever::set_result(serde_json::json!({ "U": u, "S": s, "V": vv }).to_string());
    }

    /// Kalman 1-D step. `args` = {"z","x","p","q","r"}.
    #[no_mangle]
    pub extern "C" fn bebop_kalman(args: *const u8, len: usize) {
        let bytes = unsafe { std::slice::from_raw_parts(args, len) };
        let v: Value = match serde_json::from_slice(bytes) { Ok(v) => v, Err(e) => { crate::retriever::set_result(format!("{{\"error\":\"{e}\"}}")); return; } };
        let g = |k: &str| v[k].as_f64().unwrap_or(0.0);
        let (x, p) = kalman_1d(g("z"), g("x"), g("p"), g("q"), g("r"));
        crate::retriever::set_result(serde_json::json!({ "x": x, "p": p }).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_line_denies_migrations_and_secrets() {
        assert!(!decide("packages/db/migrations/002_users.sql", "edit", &[], &[], "").ok);
        assert!(!decide(".env", "read", &[], &[], "").ok);
        assert!(!decide("secret/key.txt", "read", &[], &[], "").ok);
    }

    #[test]
    fn green_allows_tool_files() {
        let d = decide("tools/bebop/src/loop.ts", "edit", &[], &["tools/bebop/**".to_string()], "/repo");
        assert!(d.ok, "tool file must be allowed");
    }

    #[test]
    fn user_deny_strengthens_not_relaxes() {
        // user deny adds a glob; a previously-allowed path is now refused
        assert!(decide("src/experimental.ts", "edit", &[], &[], "").ok);
        assert!(!decide("src/experimental.ts", "edit", &["**/experimental.ts".into()], &[], "").ok);
    }

    #[test]
    fn scope_blocks_outside_surface() {
        let d = decide("apps/api/server.ts", "edit", &[], &["tools/bebop/**".to_string()], "/repo");
        assert!(!d.ok);
        assert_eq!(d.kind, "scope");
    }

    #[test]
    fn retriever_embed_is_deterministic_and_similar() {
        let a = retriever::embed("the red ship lifts off", 256);
        let b = retriever::embed("the red ship lifts off", 256);
        let c = retriever::embed("unrelated coffee morning", 256);
        assert_eq!(a, b, "same input → same vector");
        assert!(retriever::similarity(&a, &b) > retriever::similarity(&a, &c));
    }

    #[test]
    fn glob_stars_and_wildcards() {
        assert!(glob::matches("**/auth/**", "x/y/auth/token.ts"));
        assert!(glob::matches("**/*.sql", "a/b/migration.sql"));
        assert!(!glob::matches("**/secret/**", "src/secret.ts")); // not a directory
    }

    #[test]
    fn log_records_decisions() {
        let before = log_len();
        let d = decide("migrations/x.sql", "edit", &[], &[], "");
        record("migrations/x.sql", "edit", &d);
        assert_eq!(log_len(), before + 1);
    }

    #[test]
    fn math_svd_reconstructs_faithfully() {
        // A = [[1,2],[3,4]] — SVD gives U·diag(S)·Vᵀ ≈ A
        let a = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        let (u, s, v) = math::svd(&a);
        // reconstruct: U * S * V^T
        let mut acc = 0.0;
        for i in 0..2 { for j in 0..2 {
            let mut r = 0.0;
            for k in 0..2 { r += u[i][k] * s[k] * v[j][k]; }
            acc += (r - a[i][j]).abs();
        } }
        assert!(acc < 1e-6, "SVD must reconstruct A (got residual {acc})");
    }

    #[test]
    fn math_kalman_converges_to_measurement() {
        // a run of identical measurements should pull the estimate toward z
        let z = 10.0;
        let mut x = 0.0; let mut p = 1.0;
        for _ in 0..200 { let (nx, np) = math::kalman_1d(z, x, p, 1e-3, 0.5); x = nx; p = np; }
        assert!((x - z).abs() < 0.05, "kalman should converge to z=10 (got {x})");
    }
}
