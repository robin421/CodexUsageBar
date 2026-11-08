//! `usagebar [--json] [--all] [--en]` — print quotas for every provider logged in on this machine.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use quota_core::format::{countdown, window_name, Lang};
use quota_core::{refresh_all, Ctx, Issue, Status};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let lang = if has("--en") { Lang::En } else { Lang::Zh };

    let ctx = Ctx::new(HashMap::new()).await;
    let reports = refresh_all(&ctx, &HashSet::new(), &HashMap::new()).await;

    if has("--json") {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
        return;
    }
    let now = Utc::now();
    for r in reports.iter().filter(|r| has("--all") || r.visible()) {
        let plan = r.snapshot.as_ref().and_then(|s| s.plan.clone()).unwrap_or_default();
        println!("\n{}  {}", r.name, plan);
        if let Some(s) = &r.snapshot {
            for w in &s.windows {
                println!(
                    "  {:<28} {:>5.1}% left   {}",
                    window_name(w, lang),
                    w.remaining_percent(),
                    countdown(w.resets_at, now, lang)
                );
            }
            for b in &s.balances {
                println!("  {:<28} {:.2} {}", b.id, b.amount, b.currency);
            }
        }
        if r.status != Status::Ok {
            match &r.issue {
                Some(Issue::Error { message }) => println!("  ! {message}"),
                Some(i) => println!("  ! {i:?}"),
                None => {}
            }
        }
    }
}
