//! 読めない引用の枠を、本文を外に出さずに数える診断（試験だけ。`#[cfg(test)]`）。
//!
//! 利用者の機械の DB の上で、いまの parser（`split_reply`）をそのまま走らせ、差出人か日時を読めない
//! 枠の「形」だけを伏字にして数える。本文の中身は 1 文字も出さない（数字→9、アドレス→A、ラテン文字の
//! 語→w、和文→N。見出しの決まり語だけ残す）。
//!
//! ```text
//! cargo test --no-run --lib        # "Executable unittests src/lib.rs (target/debug/deps/rondine_lib-<hash>)"
//! RONDINE_DIAG_DB=<mail.db の場所> [RONDINE_DIAG_ACCOUNT=<アカウント id>] \
//!   rondine_lib-<hash> services::quotes::diag::dump_unreadable_blocks --exact --ignored --nocapture
//! ```
//!
//! DB は読み取り専用で開く（中身は変えない）。WAL の DB で `-shm` / `-wal` が無いときは、SQLite が
//! 空のものを作る（アプリが動いていれば既にある）。

use std::collections::HashMap;

use rusqlite::{params, Connection, OpenFlags};

use super::split_reply;

/// 残すラテン文字の決まり語（大文字小文字は区別しない）。見出し・属性行・日付の手がかりになる語。
const LATIN_KEEP: &[&str] = &[
    "from",
    "sent",
    "date",
    "to",
    "cc",
    "bcc",
    "subject",
    "reply",
    "wrote",
    "on",
    "at",
    "original",
    "message",
    "forwarded",
    "begin",
    "re",
    "fw",
    "fwd",
    "am",
    "pm",
    "gmt",
    "jst",
    "utc",
    "mailto",
    "a",
    "écrit",
    "le",
    "jan",
    "feb",
    "mar",
    "apr",
    "may",
    "jun",
    "jul",
    "aug",
    "sep",
    "oct",
    "nov",
    "dec",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "mon",
    "tue",
    "wed",
    "thu",
    "fri",
    "sat",
    "sun",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// 残す和文の決まり語（長いものから順に照らす）。
const JA_KEEP: &[&str] = &[
    "さんが書きました",
    "さんは書きました",
    "が書きました",
    "書きました",
    "転送されたメール",
    "引用元メッセージ",
    "元のメッセージ",
    "転送メッセージ",
    "のメール",
    "送信日時",
    "差出人",
    "送信者",
    "送信元",
    "宛先",
    "件名",
    "日付",
    "日時",
    "送信",
    "午前",
    "午後",
    "写道",
    "发件人",
    "年",
    "月",
    "日",
    "時",
    "分",
];

/// 和文の字か（ひらがな・カタカナ・漢字・半角カナ・々〆ー）。
fn is_japanese(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}' | '\u{FF66}'..='\u{FF9F}' | '々' | '〆')
}

/// アドレスに使われる字か（`@` の前後を広げるのに使う）。
fn is_addr_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// アドレス（`@` を挟んだ字の並び）を `A` に置き換える。
fn mask_addresses(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            // 直前に出した「アドレスの字」を取り消して A にする。
            while out.chars().last().is_some_and(is_addr_char) {
                out.pop();
            }
            out.push('A');
            i += 1;
            while i < chars.len() && is_addr_char(chars[i]) {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 和文の続きを伏字にする（決まり語は残し、それ以外の続きは `N` 1 つにまとめる）。
fn mask_japanese_run(run: &str, out: &mut String) {
    let mut rest = run;
    while let Some(c) = rest.chars().next() {
        if let Some(word) = JA_KEEP.iter().find(|w| rest.starts_with(**w)) {
            out.push_str(word);
            rest = &rest[word.len()..];
            continue;
        }
        if !out.ends_with('N') {
            out.push('N');
        }
        rest = &rest[c.len_utf8()..];
    }
}

/// 1 行を伏字にする。数字の続き→`9`、アドレス→`A`、ラテン文字の語→`w`（決まり語は残す）、
/// 和文→`N`（決まり語は残す）、ほかの文字の語→`w`。記号と空白は残す。長い行は 100 字で切る。
pub(super) fn mask_line(line: &str) -> String {
    let line = mask_addresses(line.trim_end());
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c == 'A'
            && (i + 1 == chars.len() || !chars[i + 1].is_alphanumeric())
            && (i == 0 || !chars[i - 1].is_alphanumeric())
        {
            // mask_addresses が置いたアドレスの印。
            out.push('A');
            i += 1;
        } else if c.is_numeric() {
            while i < chars.len() && chars[i].is_numeric() {
                i += 1;
            }
            out.push('9');
        } else if is_japanese(c) {
            while i < chars.len() && is_japanese(chars[i]) {
                i += 1;
            }
            mask_japanese_run(&chars[start..i].iter().collect::<String>(), &mut out);
        } else if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() && !is_japanese(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let lower = word.to_lowercase();
            if LATIN_KEEP.contains(&lower.as_str()) {
                out.push_str(&word);
            } else {
                out.push('w');
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.chars().take(100).collect()
}

/// 枠の形の鍵。直前の空でない 2 行と、枠の先頭の空でない 3 行を伏字にしてつなぐ。
fn shape_key(before: &[&str], head: &[&str]) -> String {
    let masked = |ls: &[&str]| {
        ls.iter()
            .map(|l| mask_line(l))
            .collect::<Vec<_>>()
            .join(" ⏎ ")
    };
    format!("{}  ‖  {}", masked(before), masked(head))
}

/// 1 通の本文から、差出人か日時を読めない枠の形を数える。数えた枠の数を返す。
fn count_unreadable(
    body: &str,
    shapes: &mut HashMap<String, usize>,
    kinds: &mut [usize; 3],
) -> usize {
    let lines: Vec<&str> = body.lines().collect();
    let mut cursor = 0;
    let mut n = 0;
    for q in split_reply(body).quotes {
        // 枠の先頭の行（見出しも含めた全文の最初の空でない行）を、本文の中で前から順に探す。
        let first = q.text.lines().map(str::trim).find(|l| !l.is_empty());
        let pos = first.and_then(|f| (cursor..lines.len()).find(|&i| lines[i].trim() == f));
        if let Some(p) = pos {
            cursor = p + 1;
        }
        if q.quoted_from.is_some() && q.quoted_at.is_some() {
            continue;
        }
        n += 1;
        let k = match (q.quoted_from.is_some(), q.quoted_at.is_some()) {
            (false, false) => 0,
            (false, true) => 1,
            _ => 2,
        };
        kinds[k] += 1;
        let head: Vec<&str> = q
            .text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(3)
            .collect();
        let before: Vec<&str> = pos
            .map(|p| {
                let mut b: Vec<&str> = lines[..p]
                    .iter()
                    .rev()
                    .filter(|l| !l.trim().is_empty())
                    .take(2)
                    .copied()
                    .collect();
                b.reverse();
                b
            })
            .unwrap_or_default();
        *shapes.entry(shape_key(&before, &head)).or_default() += 1;
    }
    n
}

/// 利用者の DB を読み取り専用で開き、読めない引用の枠の形を数えて、上位 80 を出す。
#[test]
#[ignore = "利用者の機械の DB が要る（RONDINE_DIAG_DB）"]
fn dump_unreadable_blocks() {
    let Ok(path) = std::env::var("RONDINE_DIAG_DB") else {
        println!("RONDINE_DIAG_DB が無いので何もしない");
        return;
    };
    let account: Option<i64> = std::env::var("RONDINE_DIAG_ACCOUNT")
        .ok()
        .and_then(|a| a.parse().ok());
    // 読み取り専用で開く（書き込み・WAL の作成もしない）。
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("RONDINE_DIAG_DB を読み取り専用で開けない");
    conn.execute_batch("PRAGMA query_only=ON;")
        .expect("query_only");
    let mut stmt = conn
        .prepare(
            "SELECT e.body_plain FROM emails e
             WHERE e.body_plain IS NOT NULL AND (?1 IS NULL OR e.account_id = ?1)
               AND EXISTS (SELECT 1 FROM message_quotes q WHERE q.email_id = e.id
                           AND (q.quoted_from IS NULL OR q.quoted_at IS NULL))",
        )
        .expect("問い合わせ");
    let bodies = stmt
        .query_map(params![account], |r| r.get::<_, String>(0))
        .expect("問い合わせ");
    let mut shapes: HashMap<String, usize> = HashMap::new();
    let mut kinds = [0usize; 3];
    let (mut mails, mut blocks) = (0, 0);
    for body in bodies {
        let body = body.expect("本文の読み出し");
        mails += 1;
        blocks += count_unreadable(&body, &mut shapes, &mut kinds);
    }
    let mut ranked: Vec<(String, usize)> = shapes.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    println!(
        "mails {mails} unreadable-blocks {blocks} (from+at null {}, from null {}, at null {}) shapes {}",
        kinds[0],
        kinds[1],
        kinds[2],
        ranked.len()
    );
    println!("== 枠の形 上位 80（直前の 2 行  ‖  枠の先頭の 3 行）");
    for (shape, count) in ranked.iter().take(80) {
        println!("{count:6}  {shape}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 伏字に本文の中身が残らない（決まり語・記号だけが残る）。
    #[test]
    fn masks_content_but_keeps_header_words() {
        assert_eq!(
            mask_line("From: 山田 花子 <hanako.yamada@example.co.jp>"),
            "From: N N <A>"
        );
        assert_eq!(
            mask_line("送信日時: 2026年10月2日 17:55"),
            "送信日時: 9年9月9日 9:9"
        );
        assert_eq!(
            mask_line("On Fri, Oct 2, 2026 at 5:55 PM Taro Suzuki wrote:"),
            "On Fri, Oct 9, 9 at 9:9 PM w w wrote:"
        );
        assert_eq!(
            mask_line("2026/10/14 12:50:57 に ayaku0119@example.jp さんが書きました:"),
            "9/9/9 9:9:9 N A さんが書きました:"
        );
        assert_eq!(mask_line("Привет мир"), "w w");
        assert_eq!(mask_line("ＡＢＣ　１２３"), "w　9");
    }

    /// 作り物の本文で、読めない枠だけを数え、直前の行も伏字で付く。
    #[test]
    fn counts_only_unreadable_blocks() {
        let body = "了解です。\n\n----- Original Message -----\n本文だけの引用\n二行目\n\n\
On Fri, Oct 2, 2026 at 5:55 PM Taro <taro@example.com> wrote:\n> 元の本文";
        let mut shapes = HashMap::new();
        let mut kinds = [0usize; 3];
        let n = count_unreadable(body, &mut shapes, &mut kinds);
        assert_eq!(n, 1);
        assert_eq!(kinds, [1, 0, 0]);
        let (shape, count) = shapes.into_iter().next().unwrap();
        assert_eq!(count, 1);
        assert_eq!(shape, "N。  ‖  ----- Original Message ----- ⏎ N ⏎ N");
    }
}
