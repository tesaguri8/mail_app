//! 差出人の認証結果（`Authentication-Results`）から「なりすましの疑い」を判定する
//! （docs/MAIL_SECURITY.md §2・§2.1）。
//!
//! 差出人の欄（From）は誰でも偽れる。グリーン・住所録・返信歴はどれも From のアドレスで判定するので、
//! 認証に落ちたメールはそれらに当たっていても信頼しない（本文のリンクを左クリックで開かせない）。
//!
//! 過剰警告を避けるため、次のときだけ疑う:
//! - DMARC が fail（From のドメインと認証が揃わなかった＝なりすましの典型）
//! - DMARC の記載が無く、SPF か DKIM が fail で、どちらも pass していない
//!
//! SPF の softfail / neutral / none は疑わない（転送やメーリングリストで普通に起きる）。

/// 認証の結果 1 つ（`spf=pass` の `pass` の部分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail,
    Other,
}

/// `Authentication-Results` の中から `<method>=<result>` を探し、最初の結果を返す（無ければ None）。
/// `header.d=` のような属性（`.` を含む）や、`dkim=failed` のような綴りの違いを取り違えない。
fn verdict(auth_result: &str, method: &str) -> Option<Verdict> {
    let lower = auth_result.to_ascii_lowercase();
    lower
        .split(|c: char| c.is_whitespace() || c == ';' || c == '(' || c == ')')
        .filter_map(|tok| tok.split_once('='))
        .find(|(name, _)| *name == method)
        .map(|(_, value)| match value {
            "pass" => Verdict::Pass,
            "fail" => Verdict::Fail,
            _ => Verdict::Other,
        })
}

/// 認証結果から、なりすましの疑いがあるか。認証結果が無ければ疑わない（判断の材料が無い）。
///
/// 引数は保存してある `emails.auth_result`（受信側 MTA が付けた `Authentication-Results`）。
pub fn spoof_suspected(auth_result: Option<&str>) -> bool {
    let Some(ar) = auth_result.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    match verdict(ar, "dmarc") {
        Some(Verdict::Fail) => true,
        Some(_) => false,
        None => {
            let spf = verdict(ar, "spf");
            let dkim = verdict(ar, "dkim");
            let failed = [spf, dkim].contains(&Some(Verdict::Fail));
            let passed = [spf, dkim].contains(&Some(Verdict::Pass));
            failed && !passed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 認証結果が無い・空なら疑わない。
    #[test]
    fn no_result_is_not_suspected() {
        assert!(!spoof_suspected(None));
        assert!(!spoof_suspected(Some("  ")));
    }

    /// DMARC が fail なら、SPF・DKIM が pass でも疑う（From と揃っていない）。
    #[test]
    fn dmarc_fail_is_suspected() {
        assert!(spoof_suspected(Some(
            "mx.example.invalid; spf=pass smtp.mailfrom=a.invalid; dkim=pass header.d=a.invalid; dmarc=fail header.from=bank.invalid"
        )));
    }

    /// 全部 pass なら疑わない。DMARC が pass なら SPF が fail でも疑わない（転送でよくある）。
    #[test]
    fn dmarc_pass_is_not_suspected() {
        assert!(!spoof_suspected(Some(
            "mx.example.invalid; spf=pass; dkim=pass; dmarc=pass"
        )));
        assert!(!spoof_suspected(Some(
            "mx.example.invalid; spf=fail; dkim=pass; dmarc=pass"
        )));
    }

    /// DMARC の記載が無いときは、SPF か DKIM が fail で、どちらも pass していなければ疑う。
    #[test]
    fn without_dmarc_needs_a_fail_and_no_pass() {
        assert!(spoof_suspected(Some("mx.example.invalid; spf=fail")));
        assert!(spoof_suspected(Some(
            "mx.example.invalid; spf=none; dkim=fail"
        )));
        assert!(!spoof_suspected(Some(
            "mx.example.invalid; spf=fail; dkim=pass"
        )));
    }

    /// softfail は疑わない（転送・メーリングリストで普通に起きる）。
    #[test]
    fn softfail_is_not_suspected() {
        assert!(!spoof_suspected(Some(
            "mx.example.invalid; spf=softfail; dkim=none"
        )));
    }

    /// 大文字・括弧の注記・属性（header.d=）に引っ張られない。
    #[test]
    fn reads_case_and_comments() {
        assert!(spoof_suspected(Some(
            "mx.example.invalid; DMARC=FAIL (p=REJECT sp=REJECT) header.from=bank.invalid"
        )));
        assert!(!spoof_suspected(Some(
            "mx.example.invalid; dkim=pass (2048-bit key) header.d=fail.invalid"
        )));
    }
}
