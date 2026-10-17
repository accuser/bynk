//! #2028: `bynk deploy --dry-run --env staging` names what a staging deploy
//! creates. The push hands Wrangler `--env staging` and an `[env.staging]`
//! block, so each Worker deploys as `<name>-staging`, binds to `-staging`
//! targets and consumes `-staging` queues; the plan, in text and JSON, says so.
//!
//! Driven through the real binary over a two-context project: `shop.orders`
//! consumes `shop.payment` and a queue. `--dry-run` compiles, checks and prints
//! the plan without touching Cloudflare.

use std::path::{Path, PathBuf};
use std::process::Command;

const PAYMENT: &str = r#"context shop.payment

exports transparent { Receipt }

type Receipt = { id: String }

service authorise {
  on call(id: String) -> Effect[Result[Receipt, Int]] {
    Ok(Receipt { id: id })
  }
}
"#;

const ORDERS: &str = r#"context shop.orders

consumes shop.payment

type Job = { id: String }

service place {
  on call(id: String) -> Effect[Result[String, Int]] {
    let receipt <- shop.payment.authorise(id)
    match receipt {
      Ok(r) => Ok(r.id)
      Err(code) => Err(code)
    }
  }
}

service intake from queue("job-intake") {
  on message(message: Job) -> Effect[QueueResult] {
    Ack
  }
}
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/shop")).unwrap();
    std::fs::write(
        dir.join("bynk.toml"),
        "[project]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/shop/payment.bynk"), PAYMENT).unwrap();
    std::fs::write(dir.join("src/shop/orders.bynk"), ORDERS).unwrap();
    dir
}

fn on_path(name: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `bynk deploy --dry-run <args>` in `dir`: (success, stdout, stderr).
fn dry_run(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bynk"))
        .args(["deploy", "--dry-run"])
        .args(args)
        .current_dir(dir)
        .env_remove("CI")
        .output()
        .expect("run bynk");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_staging_plan_names_the_staging_workers_bindings_and_queues() {
    // `deploy`'s preflight needs a way to run wrangler, though a dry run never
    // calls it.
    if !on_path("wrangler") && !on_path("npx") {
        eprintln!("\n!!! DEPLOY ENV SKIPPED !!!\nneither `wrangler` nor `npx` is on PATH\n");
        return;
    }
    let dir = scratch("deploy-env-staging");

    let (ok, text, err) = dry_run(&dir, &["--env", "staging"]);
    assert!(ok, "{text}{err}");
    for line in [
        "deploy shop-payment-staging\n",
        "queue create job-intake-staging\n",
        "deploy shop-orders-staging\n",
        "order shop-payment-staging → shop-orders-staging\n",
    ] {
        assert!(text.contains(line), "missing `{}`:\n{text}", line.trim());
    }
    assert!(
        !text.contains("deploy shop-orders\n"),
        "an unsuffixed Worker:\n{text}"
    );

    let (ok, json, err) = dry_run(&dir, &["--env", "staging", "--format", "json"]);
    assert!(ok, "{json}{err}");
    let plan: serde_json::Value = serde_json::from_str(&json).expect("the plan is JSON");
    assert_eq!(plan["environment"], "staging");
    assert_eq!(
        plan["order"],
        serde_json::json!(["shop-payment-staging", "shop-orders-staging"])
    );
    let orders = &plan["contexts"][1];
    assert_eq!(orders["worker"], "shop-orders-staging");
    assert_eq!(
        orders["binds_to"],
        serde_json::json!(["shop-payment-staging"])
    );
    assert_eq!(orders["queues"][0]["queue"], "job-intake-staging");

    // The default environment is unchanged: bare names throughout.
    let (ok, text, err) = dry_run(&dir, &[]);
    assert!(ok, "{text}{err}");
    assert!(text.contains("queue create job-intake\n"), "{text}");
    assert!(
        text.contains("order shop-payment → shop-orders\n"),
        "{text}"
    );
}
