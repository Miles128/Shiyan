//! Manual probe, not part of a normal `cargo test` run: compares what the real
//! extractor keeps with what the raw page holds, to tell "the source has no
//! article" apart from "we threw most of it away".
//!
//! Reads URLs from `SHIYAN_PROBE_FILE`, one per line:
//!
//! ```sh
//! SHIYAN_PROBE_FILE=/tmp/urls.txt cargo test -p shiyan-core probe -- --ignored --nocapture
//! ```

use super::extract;
use super::net;
use super::{html_to_text, MIN_ARTICLE_WORDS};

#[test]
#[ignore = "对指定 URL 跑真实抽取器，只在人工核对丢文原因时执行"]
fn probe_extractor_word_counts() {
    let path = std::env::var("SHIYAN_PROBE_FILE").expect("SHIYAN_PROBE_FILE");
    let urls = std::fs::read_to_string(&path).expect("read url list");
    println!("\n抽取正文词数\t整页可见词数\t判定\t链接");
    for url in urls.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let client = net::http_client().expect("http client");
        let whole = extract::extract_page(&client, url).ok().map(|p| p.text);
        let fetched = (|| {
            let resp = client.get(url).send().ok()?.error_for_status().ok()?;
            Some(String::from_utf8_lossy(&net::read_limited_bytes(resp).ok()?).into_owned())
        })();
        let whole_page = fetched.as_deref().map(html_to_text);
        let (gotten, page) = match (&whole, &whole_page) {
            (Some(t), Some(p)) => (t.split_whitespace().count(), p.split_whitespace().count()),
            _ => {
                println!("-\t-\t页面抓取失败\t{url}");
                continue;
            }
        };
        let verdict = if gotten >= MIN_ARTICLE_WORDS {
            "可入库"
        } else if page >= MIN_ARTICLE_WORDS {
            "被丢，但整页文本够长 → 抽取丢文"
        } else {
            "被丢，整页也确实短"
        };
        println!("{gotten}\t{page}\t{verdict}\t{url}");
    }
}
