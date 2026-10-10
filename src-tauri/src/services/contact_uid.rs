//! 連絡先の uid（端末をまたいで同じ人を指す ID）。docs/CONTACT_MODEL.md §1-1。
//!
//! DB の `contacts.uid` は UUID v4（小文字・ハイフン区切り）で、マイグレーション 0064 のトリガーが
//! 振る。vCard の UID として書き出し、取り込みで同じ uid の人を同じ人として扱う。

/// 連絡先の uid（UUID・小文字ハイフン区切り）。[`ContactUid::parse`] でしか作れない。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContactUid(String);

impl ContactUid {
    /// vCard の UID や DB の値から読む。`urn:uuid:` の接頭辞・大文字・前後の空白を吸収する。
    ///
    /// UUID の形（8-4-4-4-12 の 16 進）でなければ None（iCloud の `…:ABPerson` や Google の
    /// 独自 ID など、Rondine が振ったものでない UID は人の照合に使わない）。版は問わない。
    pub fn parse(raw: &str) -> Option<Self> {
        let t = raw.trim();
        let body = t
            .get(..9)
            .filter(|p| p.eq_ignore_ascii_case("urn:uuid:"))
            .map_or(t, |_| &t[9..]);
        let lower = body.to_ascii_lowercase();
        let groups: Vec<&str> = lower.split('-').collect();
        let shape_ok = groups.len() == 5
            && groups
                .iter()
                .zip([8, 4, 4, 4, 12])
                .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()));
        shape_ok.then_some(Self(lower))
    }

    /// 小文字ハイフン区切りの文字列。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_urn_and_uppercase() {
        let want = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(ContactUid::parse(want).unwrap().as_str(), want);
        assert_eq!(
            ContactUid::parse("urn:uuid:0F8FAD5B-D9CB-469F-A165-70867728950E")
                .unwrap()
                .as_str(),
            want
        );
        assert_eq!(
            ContactUid::parse(&format!(" URN:UUID:{want} "))
                .unwrap()
                .as_str(),
            want
        );
    }

    #[test]
    fn rejects_foreign_ids() {
        assert!(ContactUid::parse("").is_none());
        assert!(ContactUid::parse("ABC-123").is_none());
        assert!(ContactUid::parse("0F8FAD5B-D9CB-469F-A165-70867728950E:ABPerson").is_none());
        assert!(ContactUid::parse("0f8fad5bd9cb469fa16570867728950e").is_none());
        assert!(ContactUid::parse("zf8fad5b-d9cb-469f-a165-70867728950e").is_none());
    }
}
