use crate::scenario::StepResult;
use serde_json::{Value, json};
use std::time::Duration;

fn secs(d: Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

fn slowest(step: &StepResult) -> String {
    step.traffic
        .latency
        .iter()
        .max_by_key(|(_, l)| l.max)
        .filter(|(_, l)| l.max >= Duration::from_millis(50))
        .map(|(m, l)| {
            let method = m.strip_prefix("textDocument/").unwrap_or(m);
            format!("{method} {}ms", l.max.as_millis())
        })
        .unwrap_or_default()
}

pub fn print_table(results: &[StepResult], threshold: f64) {
    println!();
    println!(
        "{:<52} {:>7} {:>8} {:>8} {:>6} {:>6} {:>7} {:>11} {:>6} {:>6} {:>5}  slowest request",
        "step",
        "action",
        "settle",
        "cpu",
        "peak",
        "idle",
        "rss",
        "watch(out)",
        "pub",
        "pulls",
        "rfsh"
    );
    let mut iteration = usize::MAX;
    for r in results {
        if r.iteration != iteration {
            iteration = r.iteration;
            if iteration > 0 {
                println!("-- iteration {iteration}");
            }
        }
        let settle = match (&r.error, r.settle, r.unsettled) {
            (Some(_), _, _) => "FAILED".to_string(),
            (_, Some(d), _) => secs(d),
            (_, None, Some(reason)) => format!("!{reason}"),
            _ => "?".to_string(),
        };
        let mut name = r.name.clone();
        if name.chars().count() > 52 {
            name = name.chars().take(51).collect::<String>() + "…";
        }
        println!(
            "{:<52} {:>7} {:>8} {:>8} {:>5.2}c {:>5.2}c {:>6.0}M {:>11} {:>6} {:>6} {:>5}  {}",
            name,
            secs(r.action),
            settle,
            secs(r.cpu),
            r.peak_cores,
            r.idle_cores,
            r.rss_mb,
            format!("{}({})", r.watch.sent, r.watch.sent_output),
            r.traffic.count("<- textDocument/publishDiagnostics"),
            r.traffic.count("-> workspace/diagnostic"),
            r.traffic.count("<- workspace/diagnostic/refresh"),
            slowest(r),
        );
    }

    println!();
    println!(
        "settle: time from the end of the action until the server stayed under {threshold:.2} cores; \
         !reason = never did"
    );
    println!("cpu: CPU seconds from the start of the step until it settled. peak: busiest 1s.");
    println!(
        "idle: cores used in the idle window after settling. pub/pulls/rfsh: publishDiagnostics, workspace/diagnostic, diagnostic refreshes."
    );
    println!("watch(out): watched-file events delivered, of which codegen output.");

    let hot: Vec<&StepResult> = results
        .iter()
        .filter(|r| r.unsettled.is_some() || r.idle_cores >= threshold)
        .collect();
    if !hot.is_empty() {
        println!();
        println!("Steps that left the server busy:");
        for r in hot {
            print!(
                "  {} (idle {:.2} cores{})",
                r.name,
                r.idle_cores,
                r.unsettled
                    .map(|u| format!(", waiting on {u}"))
                    .unwrap_or_default()
            );
            match &r.sample {
                Some(p) => println!(" — stacks in {}", p.display()),
                None => println!(),
            }
        }
    }
    for r in results.iter().filter(|r| r.error.is_some()) {
        println!("  {} failed: {}", r.name, r.error.as_deref().unwrap_or(""));
    }
}

pub fn to_json(results: &[StepResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "iteration": r.iteration,
                    "error": r.error,
                    "action_s": r.action.as_secs_f64(),
                    "settle_s": r.settle.map(|d| d.as_secs_f64()),
                    "unsettled": r.unsettled,
                    "cpu_s": r.cpu.as_secs_f64(),
                    "peak_cores": r.peak_cores,
                    "idle_cores": r.idle_cores,
                    "rss_mb": r.rss_mb,
                    "threads": r.threads,
                    "watch": {
                        "sent": r.watch.sent,
                        "sent_output": r.watch.sent_output,
                        "unmatched": r.watch.unmatched,
                        "notifications": r.watch.notifications,
                    },
                    "messages": r.traffic.counts,
                    "latency_ms": r.traffic.latency.iter().map(|(m, l)| (m.clone(), json!({
                        "count": l.count,
                        "mean": l.total.as_secs_f64() * 1000.0 / l.count.max(1) as f64,
                        "max": l.max.as_secs_f64() * 1000.0,
                    }))).collect::<serde_json::Map<_, _>>(),
                    "cancelled": r.traffic.cancelled,
                    "log_warnings": r.traffic.log_warnings,
                    "log_errors": r.traffic.log_errors,
                    "sample": r.sample,
                })
            })
            .collect(),
    )
}
