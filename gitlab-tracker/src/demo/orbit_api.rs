//! Orbit API: the back end of a parcel-delivery platform (Rust, Axum,
//! Postgres). Twelve weeks of history: a release to production every week,
//! the main branch promoted to staging every other day, a reverted change, a
//! hotfix picked straight to production, and sixteen MRs in review, one for
//! each state an MR can be in.

use super::clock::{ahead, d, h, m, When};
use super::model::Ev::*;
use super::model::*;
use crate::models::MergeabilityStatus as S;

pub const PROJECT_ID: &str = "4217";

pub static THOMAS: Person = Person {
    name: "Thomas Dubosc",
    login: "thomas_db",
};
pub static ALEX: Person = Person {
    name: "Alex Devries",
    login: "alex_dev",
};
pub static SARAH: Person = Person {
    name: "Sarah Codewyn",
    login: "sarah_code",
};
pub static JULIEN: Person = Person {
    name: "Julien Morel",
    login: "julien_m",
};
pub static MARINA: Person = Person {
    name: "Marina Graphetti",
    login: "marina_gql",
};
pub static NADIA: Person = Person {
    name: "Nadia Okafor",
    login: "nadia_ok",
};
pub static RENOVATE: Person = Person {
    name: "Renovate Bot",
    login: "renovate_bot",
};

/// "Me" in the demo.
pub const ME: &str = "thomas_db";

pub fn spec() -> ProjectSpec {
    let mut syncs = Vec::new();
    // main → staging every other working day, staging → production weekly.
    for n in (1..=59).step_by(2) {
        syncs.push(Sync {
            at: d(n, 10, 30),
            from: "main",
            to: "staging",
            by: &NADIA,
            // The registry timed out yesterday: staging still runs the build before.
            deploy: if n == 1 { Deploy::Failed } else { Deploy::Ok },
        });
    }
    for n in (3..=58).step_by(5) {
        syncs.push(Sync {
            at: d(n, 15, 30),
            from: "staging",
            to: "production",
            by: &NADIA,
            // Migration 46 hit a lock timeout; run again once traffic dropped.
            deploy: if n == 13 {
                Deploy::FailedThenOk(95)
            } else {
                Deploy::Ok
            },
        });
    }
    let mut mrs = history();
    mrs.extend(in_review());
    ProjectSpec {
        id: PROJECT_ID,
        name: "Orbit API",
        path: "orbit/orbit-api",
        branches: vec!["main", "staging", "production"],
        flow: None,
        environments: vec![
            ("main", "development"),
            ("staging", "staging"),
            ("production", "production"),
        ],
        jobs: vec![
            ("lint", "test", 52.0),
            ("unit-tests", "test", 168.0),
            ("integration-tests", "test", 284.0),
            ("api-contract", "test", 121.0),
            ("build-image", "build", 196.0),
        ],
        first_iid: 168,
        mrs,
        syncs,
        copies: vec![
            CopySpec {
                at: d(22, 10, 5),
                mr: "revert-lru",
                to: "production",
                how: CopyHow::CherryPick,
                by: &THOMAS,
            },
            CopySpec {
                at: d(2, 16, 40),
                mr: "migration-null",
                to: "production",
                how: CopyHow::Plain,
                by: &NADIA,
            },
            CopySpec {
                at: d(2, 17, 30),
                mr: "csv-export",
                to: "production",
                how: CopyHow::Partial(2),
                by: &THOMAS,
            },
            CopySpec {
                at: d(1, 11, 45),
                mr: "hotfix-refresh",
                to: "production",
                how: CopyHow::CherryPick,
                by: &NADIA,
            },
        ],
        directs: vec![
            Direct {
                at: d(13, 16, 5),
                branch: "main",
                title: "chore(release): Start 2.4.0-dev",
                by: &NADIA,
            },
            Direct {
                at: d(4, 12, 10),
                branch: "main",
                title: "docs: Fix the local setup steps in the README",
                by: &THOMAS,
            },
        ],
        branch_ci: vec![("main", Ci::Running)],
        milestones: vec![
            MilestoneSpec {
                title: "v2.3.0",
                due: Some(d(13, 18, 0)),
                description: "October release: public tracking page, label templates per country, webhook retries.",
            },
            MilestoneSpec {
                title: "Security audit",
                due: Some(d(5, 18, 0)),
                description: "Findings of the September penetration test, to fix before the partners' review.",
            },
            MilestoneSpec {
                title: "v2.4.0",
                due: Some(ahead(7, 18, 0)),
                description: "GraphQL for the partners, OAuth for the mobile app, rate limiting.",
            },
            MilestoneSpec {
                title: "v2.5.0",
                due: Some(ahead(17, 18, 0)),
                description: "Push notifications and live courier tracking.",
            },
            MilestoneSpec {
                title: "Tech debt",
                due: None,
                description: "Clean-ups taken when a sprint has room.",
            },
        ],
        tickets: tickets(),
        rules: vec![
            RuleSpec {
                name: "Maintainers",
                required: 2,
                eligible: vec![&THOMAS, &MARINA, &ALEX, &SARAH, &JULIEN],
                paths: Vec::new(),
            },
            RuleSpec {
                name: "Database",
                required: 1,
                eligible: vec![&NADIA, &THOMAS],
                paths: vec!["migrations/", "src/db/"],
            },
        ],
        last_session: d(2, 18, 30),
        profile: crate::models::DifficultyProfile {
            name: "Rust API".into(),
            easy_threshold: 30,
            hard_threshold: 200,
        },
    }
}

/// A bot's MR: opened at night, approved by two maintainers, merged by the first.
fn bot(
    key: &'static str,
    title: &'static str,
    opened: When,
    approved: When,
    merged: When,
) -> MrSpec {
    MrSpec::new(key, title, &RENOVATE)
        .assignee(&THOMAS)
        .labels(&["dependencies"])
        .at(opened, Open)
        .at(opened.plus(1), Push(leak_one(title), Ci::Pass))
        .at(approved, Approve(&THOMAS))
        .at(approved.plus(25), Approve(&ALEX))
        .at(merged, Merge(&THOMAS))
        .status(S::NotOpen)
}

/// The merged MRs of the last twelve weeks, oldest first.
fn history() -> Vec<MrSpec> {
    vec![
        MrSpec::new("shipments-v2", "feat(api): Shipments v2 endpoints", &MARINA)
            .branch("feat/3902-shipments-v2", "main")
            .labels(&["feature", "api"])
            .about("The v2 list and detail endpoints for shipments: cursor-ready, with the carrier and the last scan inlined.")
            .file(patch("src/api/shipments/handler.rs", &[(12, r#"
 use crate::db::shipments::{self, ShipmentFilter};
+use crate::api::v2::ShipmentView;

 pub async fn list(State(db): State<Db>, Query(filter): Query<ShipmentFilter>) -> ApiResult<Json<Vec<ShipmentView>>> {
-    let rows = shipments::list(&db, &filter).await?;
-    Ok(Json(rows.into_iter().map(Into::into).collect()))
+    let rows = shipments::list(&db, &filter.without_archived()).await?;
+    let views = rows.into_iter().map(ShipmentView::with_last_scan).collect();
+    Ok(Json(views))
 }
"#)]))
            .shipped(&[&THOMAS, &ALEX], d(58, 9, 20), d(57, 11, 0), d(56, 10, 10), d(56, 11, 40),
                ("`GET /v2/shipments` returns the archived ones too: is that wanted?", "No: they are left out now, with a test."),
                Some("fix(api): Leave archived shipments out of the v2 list")),
        MrSpec::new("duplicate-scans", "fix(scan): Duplicate scans when the handheld retries", &JULIEN)
            .branch("fix/3921-duplicate-scans", "main")
            .labels(&["bug"])
            .about("Handhelds retry a scan when the network drops after the server answered: the scan was stored twice. Scans now carry an idempotency key.")
            .file(patch("src/scan/ingest.rs", &[(40, r#"
 pub async fn ingest(db: &Db, scan: ScanEvent) -> Result<Stored> {
+    if let Some(previous) = db.scan_by_key(&scan.idempotency_key).await? {
+        return Ok(Stored::Duplicate(previous.id));
+    }
     let id = db.insert_scan(&scan).await?;
     Ok(Stored::New(id))
 }
"#)]))
            .shipped(&[&SARAH, &THOMAS], d(55, 14, 0), d(54, 9, 30), d(54, 15, 0), d(54, 15, 20),
                ("Is the idempotency key kept long enough for a retry after the handheld reboots?", "24 h, the same as the handheld's offline queue."),
                None),
        bot("sqlx-0-8-2", "chore(deps): Update sqlx to 0.8.2", d(52, 5, 40), d(51, 9, 10), d(51, 9, 40))
            .branch("renovate/sqlx-0.x", "main")
            .file(patch("Cargo.toml", &[(31, r#"
 serde_json = "1"
-sqlx = { version = "0.8.1", features = ["runtime-tokio", "postgres", "chrono", "uuid"] }
+sqlx = { version = "0.8.2", features = ["runtime-tokio", "postgres", "chrono", "uuid"] }
 thiserror = "1"
"#)]))
            .file(cargo_lock(2210, &[("sqlx", "0.8.1", "0.8.2"), ("sqlx-core", "0.8.1", "0.8.2"), ("sqlx-macros", "0.8.1", "0.8.2"), ("sqlx-macros-core", "0.8.1", "0.8.2"), ("sqlx-postgres", "0.8.1", "0.8.2")])),
        MrSpec::new("island-rates", "feat(pricing): Zone-based rates for the islands", &SARAH)
            .branch("feat/3940-island-rates", "main")
            .labels(&["feature", "pricing"])
            .about("Deliveries to the islands are priced by zone instead of by distance: the ferry is most of the cost.")
            .file(patch("src/pricing/zones.rs", &[(8, r#"
 pub fn zone_of(postcode: &str) -> Zone {
+    if ISLAND_PREFIXES.iter().any(|p| postcode.starts_with(p)) {
+        return Zone::Island;
+    }
     match &postcode[..2] {
"#), (61, r#"
-const ISLAND_PREFIXES: [&str; 3] = ["17111", "17310", "56360"];
+const ISLAND_PREFIXES: [&str; 5] = ["17111", "17310", "56360", "200", "201"];
"#)]))
            .shipped(&[&MARINA, &ALEX], d(50, 10, 0), d(49, 14, 0), d(47, 11, 0), d(47, 11, 30),
                ("Corsica is missing from the island list.", "Added, with the 200xx and 201xx postcodes."),
                Some("fix(pricing): Corsica in the island zone")),
        MrSpec::new("status-machine", "refactor(core): Shipment status as a state machine", &JULIEN)
            .branch("refactor/status-machine", "main")
            .milestone("v2.3.0")
            .labels(&["refactoring"])
            .about("The status transitions lived in four handlers. They are one table now, checked in one place.")
            .file(patch("src/core/status.rs", &[(20, r#"
 impl ShipmentStatus {
-    pub fn can_become(self, _next: ShipmentStatus) -> bool {
-        true
+    pub fn can_become(self, next: ShipmentStatus) -> bool {
+        use ShipmentStatus::*;
+        matches!(
+            (self, next),
+            (Created, PickedUp) | (PickedUp, InTransit) | (InTransit, OutForDelivery)
+                | (OutForDelivery, Delivered) | (OutForDelivery, InTransit) | (_, Returned)
+        ) && self != Delivered
     }
"#)]))
            .shipped(&[&MARINA, &THOMAS], d(46, 9, 0), d(44, 10, 30), d(42, 16, 0), d(42, 16, 20),
                ("`Delivered → InTransit` is allowed: a delivered parcel cannot leave again.", "Removed, and the transition table is tested exhaustively."),
                Some("fix(core): No transition out of Delivered")),
        MrSpec::new("public-tracking", "feat(tracking): Public tracking page API", &MARINA)
            .branch("feat/4012-public-tracking", "main")
            .milestone("v2.3.0")
            .labels(&["feature", "api"])
            .about("`GET /public/track/{number}`: what the recipient may see, without an account. No addresses, no names.")
            .file(added("src/api/public.rs", r#"
use axum::extract::{Path, State};

/// What a recipient sees: the status and the scans' places, nothing personal.
pub async fn track(State(db): State<Db>, Path(number): Path<String>) -> ApiResult<Json<PublicTracking>> {
    let shipment = db.shipment_by_number(&number).await?.ok_or(ApiError::NotFound)?;
    let scans = db.scans(shipment.id).await?;
    Ok(Json(PublicTracking::from_parts(&shipment, &scans)))
}
"#))
            .shipped(&[&ALEX, &THOMAS], d(44, 9, 40), d(43, 10, 0), d(41, 14, 30), d(41, 15, 0),
                ("Rate-limit this one: it is public and takes any tracking number.", "Behind the edge rate limiter: see the ingress rule linked in the description."),
                None),
        MrSpec::new("fuel-surcharge", "fix(pricing): Fuel surcharge applied twice on returns", &SARAH)
            .branch("fix/4031-fuel-surcharge", "main")
            .milestone("v2.3.0")
            .labels(&["bug", "pricing"])
            .about("A return reuses the outbound quote, which already includes the surcharge.")
            .file(patch("src/pricing/quote.rs", &[(77, r#"
     let mut total = base + options;
-    total += fuel_surcharge(total, carrier);
+    if !shipment.is_return() {
+        total += fuel_surcharge(total, carrier);
+    }
     Ok(Quote { total, currency })
"#)]))
            .shipped(&[&THOMAS, &MARINA], d(40, 11, 0), d(39, 9, 20), d(39, 10, 0), d(39, 10, 5),
                ("Does the credit note need the same fix?", "It reads the same total: covered by the new test."),
                None),
        MrSpec::new("webhook-retries", "feat(webhooks): Retry failed deliveries with exponential backoff", &JULIEN)
            .branch("feat/4040-webhook-retries", "main")
            .milestone("v2.3.0")
            .labels(&["feature"])
            .about("A partner's endpoint down for a minute lost every event of that minute. Deliveries are retried now.")
            .file(added("src/webhooks/retry.rs", r#"
use std::time::Duration;

/// When to try a failed delivery again: 1, 2, 4… minutes, at most an hour apart.
pub fn next_attempt(attempt: u32) -> Option<Duration> {
    (attempt < MAX_ATTEMPTS).then(|| Duration::from_secs(60 * 2u64.pow(attempt).min(60)))
}

pub const MAX_ATTEMPTS: u32 = 8;
"#))
            .shipped(&[&THOMAS, &ALEX], d(38, 9, 30), d(36, 15, 0), d(34, 10, 0), d(34, 11, 15),
                ("Cap the backoff: 2^10 minutes is 17 hours.", "Capped at an hour, 8 attempts at most."),
                Some("fix(webhooks): Cap the backoff at one hour")),
        MrSpec::new("partition-scans", "chore(db): Partition scan_events by month", &NADIA)
            .branch("chore/partition-scan-events", "main")
            .milestone("v2.3.0")
            .labels(&["database"])
            .about("`scan_events` is 900 M rows: the monthly partitions keep the indexes in memory.")
            .file(added("migrations/0047_partition_scan_events.sql", r#"
ALTER TABLE scan_events RENAME TO scan_events_legacy;
CREATE TABLE scan_events (LIKE scan_events_legacy INCLUDING ALL) PARTITION BY RANGE (scanned_at);
SELECT partman.create_parent('public.scan_events', 'scanned_at', 'native', 'monthly');
"#))
            .file(patch("src/db/scans.rs", &[(15, r#"
-const TABLE: &str = "scan_events";
+/// Partitioned by month on `scanned_at`: always filter on it.
+const TABLE: &str = "scan_events";
"#)]))
            .shipped(&[&THOMAS, &MARINA], d(33, 9, 0), d(32, 14, 0), d(30, 10, 0), d(30, 10, 20),
                ("Is the old table kept until the backfill is checked?", "Renamed to scan_events_legacy, dropped in a later migration."),
                None),
        MrSpec::new("lru-cache", "perf(cache): LRU eviction for the quote cache", &ALEX)
            .branch("perf/quote-cache-lru", "main")
            .milestone("v2.3.0")
            .labels(&["performance"])
            .about("The quote cache grew without bound. It keeps the 10 000 most recent quotes now.")
            .file(patch("src/pricing/cache.rs", &[(9, r#"
-pub type QuoteCache = DashMap<QuoteKey, Quote>;
+pub type QuoteCache = Mutex<LruCache<QuoteKey, Quote>>;
+
+pub fn quote_cache() -> QuoteCache {
+    Mutex::new(LruCache::new(NonZeroUsize::new(10_000).unwrap()))
+}
"#)]))
            .shipped(&[&MARINA, &SARAH], d(29, 10, 0), d(28, 11, 0), d(26, 9, 30), d(26, 9, 45),
                ("What happens when two requests miss the same key at once?", "Both compute it and the second insert wins: cheap enough for quotes."),
                None),
        MrSpec::new("revert-lru", "Revert \"perf(cache): LRU eviction for the quote cache\"", &THOMAS)
            .branch("revert-quote-cache-lru", "main")
            .milestone("v2.3.0")
            .labels(&["revert"])
            .about("The single mutex serialises every quote: /quotes went from 180 ms to 4 s at p95 after yesterday's release.\n\nThis reverts the merge of the LRU cache.")
            .file(patch("src/pricing/cache.rs", &[(9, r#"
-pub type QuoteCache = Mutex<LruCache<QuoteKey, Quote>>;
-
-pub fn quote_cache() -> QuoteCache {
-    Mutex::new(LruCache::new(NonZeroUsize::new(10_000).unwrap()))
-}
+pub type QuoteCache = DashMap<QuoteKey, Quote>;
"#)]))
            .shipped(&[&ALEX, &MARINA], d(22, 9, 10), d(22, 9, 20), d(22, 9, 28), d(22, 9, 35),
                ("p95 of /quotes is 4 s since the release: approving the revert.", "Thanks: I'm picking it to production right away."),
                None),
        MrSpec::new("stable-cursor", "fix(api): Paginate /shipments with a stable cursor", &JULIEN)
            .branch("fix/4058-stable-cursor", "main")
            .milestone("v2.3.0")
            .labels(&["bug", "api"])
            .about("Offsets skipped shipments created while a partner paged through the list.")
            .file(patch("src/db/shipments.rs", &[(102, r#"
-    query.push(" ORDER BY created_at DESC OFFSET ").push_bind(page.offset);
+    if let Some(after) = page.after {
+        query.push(" AND (created_at, id) < (").push_bind(after.created_at).push(", ").push_bind(after.id).push(")");
+    }
+    query.push(" ORDER BY created_at DESC, id DESC");
     query.push(" LIMIT ").push_bind(page.limit);
"#)]))
            .shipped(&[&ALEX, &THOMAS], d(24, 14, 0), d(23, 10, 0), d(20, 11, 0), d(20, 11, 10),
                ("Two shipments created in the same millisecond get skipped: sort on (created_at, id).", "Done, with a test creating 500 at once."),
                Some("fix(api): Tie-break the cursor on the id")),
        MrSpec::new("label-templates", "feat(labels): Carrier label templates per country", &MARINA)
            .branch("feat/4066-label-templates", "main")
            .milestone("v2.3.0")
            .labels(&["feature"])
            .about("Each carrier and country gets its own label layout; the customs block is added outside the EU.")
            .file(patch("src/labels/templates.rs", &[(30, r#"
 pub fn template_for(carrier: Carrier, country: CountryCode) -> Template {
-    Template::default_for(carrier)
+    let mut template = Template::default_for(carrier);
+    if !country.is_eu() {
+        template.add(Block::Customs);
+    }
+    template
 }
"#)]))
            .shipped(&[&SARAH, &ALEX], d(27, 9, 0), d(25, 14, 0), d(19, 16, 0), d(19, 16, 30),
                ("The Swiss template has no customs declaration block.", "Added: CH, NO and GB get it now."),
                Some("fix(labels): Customs block outside the EU")),
        MrSpec::new("argon2-keys", "fix(auth): Hash API keys with Argon2id", &ALEX)
            .branch("fix/4101-argon2-api-keys", "main")
            .milestone("Security audit")
            .labels(&["security"])
            .about("Finding #3 of the audit: API keys were stored as SHA-256 without salt.")
            .file(patch("src/auth/api_key.rs", &[(24, r#"
 pub fn hash(key: &str) -> String {
-    hex::encode(Sha256::digest(key.as_bytes()))
+    let salt = SaltString::generate(&mut OsRng);
+    Argon2::default().hash_password(key.as_bytes(), &salt).expect("hashing never fails").to_string()
 }
"#)]))
            .shipped(&[&THOMAS, &SARAH], d(25, 9, 30), d(24, 10, 0), d(21, 14, 0), d(21, 14, 30),
                ("The existing keys: are they rehashed on next use?", "Yes, on their first successful use; a column flags the old ones."),
                Some("feat(auth): Rehash legacy keys on first use")),
        MrSpec::new("no-stack-traces", "fix(api): Stop leaking stack traces in 500 responses", &SARAH)
            .branch("fix/4102-no-stack-traces", "main")
            .milestone("Security audit")
            .labels(&["security", "api"])
            .about("Finding #5 of the audit: a 500 returned the panic message and its backtrace.")
            .file(patch("src/api/error.rs", &[(48, r#"
             ApiError::Internal(e) => {
-                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:?}") })))
+                tracing::error!(error = ?e, %request_id, "internal error");
+                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal error", "request_id": request_id })))
             }
"#)]))
            .shipped(&[&ALEX, &MARINA], d(18, 10, 0), d(17, 9, 0), d(17, 11, 0), d(16, 9, 30),
                ("Keep the request id in the body: support asks customers for it.", "Kept, as `request_id`."),
                Some("fix(api): Keep the request id in error bodies")),
        MrSpec::new("relay-points", "chore(data): Import the 2 400 relay points", &NADIA)
            .branch("chore/import-relay-points", "main")
            .labels(&["database", "data"])
            .about("The relay points of the new partner network, from their export of October. Recipients can pick one as a delivery address.")
            .file(relay_points())
            .shipped(&[&THOMAS, &ALEX], d(18, 9, 30), d(17, 14, 0), d(16, 10, 0), d(16, 10, 20),
                ("Coordinates have 5 decimals: about a metre, enough for the map.", "Yes, the partner's own precision."),
                None),
        MrSpec::new("openapi-v2", "docs(api): OpenAPI 3.1 description of the v2 API", &MARINA)
            .branch("docs/openapi-v2", "main")
            .milestone("v2.4.0")
            .labels(&["documentation", "api"])
            .about("The partners generate their clients from it; the contract tests check the API against it.")
            .file(openapi_spec())
            .shipped(&[&THOMAS, &ALEX], d(14, 9, 0), d(13, 11, 0), d(12, 15, 0), d(12, 15, 30),
                ("`Shipment.eta` can be null before pickup: mark it `nullable`.", "Done, and `scans[].depot` too."),
                Some("docs(api): Nullable ETA and depot")),
        MrSpec::new("v2-snapshots", "test(api): Snapshot the v2 responses", &SARAH)
            .branch("test/v2-snapshots", "main")
            .milestone("v2.4.0")
            .labels(&["tests"])
            .about("A change to a v2 response now shows in the review as a snapshot diff, before a partner finds it.")
            .file(added("tests/api_v2.rs", r#"
#[tokio::test]
async fn v2_responses_do_not_change() {
    let app = TestApp::with_fixtures("shipments.sql").await;
    for (name, path) in V2_ROUTES {
        insta::assert_json_snapshot!(*name, app.get(path).await.json::<serde_json::Value>());
    }
}
"#))
            .file(snapshots())
            .shipped(&[&ALEX, &MARINA], d(9, 10, 0), d(8, 9, 30), d(7, 11, 0), d(7, 11, 20),
                ("Redact the dates, or every snapshot changes each day.", "Redacted with `[date]`."),
                Some("test(api): Redact dates in the snapshots")),
        MrSpec::new("delay-emails", "feat(notif): Email the sender when a parcel is delayed", &JULIEN)
            .branch("feat/4120-delay-emails", "main")
            .milestone("v2.4.0")
            .labels(&["feature"])
            .about("Senders learnt about delays from their customers. They get an email when the ETA slips by more than a day.")
            .file(added("src/notify/delay.rs", r#"
/// An email to the sender when the ETA slips by more than a day, once a day at most.
pub async fn on_eta_change(ctx: &Ctx, shipment: &Shipment, old: Eta, new: Eta) -> Result<()> {
    if new.date - old.date < Duration::days(1) || ctx.already_notified_today(shipment.id).await? {
        return Ok(());
    }
    ctx.mailer.send(templates::delay(shipment, new)).await
}
"#))
            .shipped(&[&THOMAS, &MARINA], d(16, 9, 0), d(15, 10, 0), d(14, 9, 30), d(14, 10, 10),
                ("Throttle these: a depot outage would send thousands.", "One email per shipment per day at most."),
                Some("fix(notif): One delay email per shipment per day")),
        bot("rustls-0-23-16", "chore(deps): Update rustls to 0.23.16 (RUSTSEC-2024-0399)", d(12, 5, 40), d(12, 9, 5), d(12, 9, 40))
            .branch("renovate/rustls-vulnerability", "main")
            .milestone("Security audit")
            .labels(&["dependencies", "security"])
            .about("This PR updates `rustls` to 0.23.16, which fixes RUSTSEC-2024-0399 (a panic on a malformed ClientHello).")
            .file(cargo_lock(1830, &[("rustls", "0.23.13", "0.23.16")])),
        MrSpec::new("eta-timezone", "fix(tracking): Courier ETA ignored the depot's time zone", &JULIEN)
            .branch("fix/4141-eta-timezone", "main")
            .milestone("v2.4.0")
            .labels(&["bug"])
            .about("Parcels from the Réunion depot showed an ETA three hours early.")
            .file(patch("src/tracking/eta.rs", &[(33, r#"
-    let opens = Local.from_local_datetime(&day.and_time(depot.opens_at)).unwrap();
+    let opens = depot.tz.from_local_datetime(&day.and_time(depot.opens_at)).unwrap();
"#)]))
            .shipped(&[&SARAH, &ALEX], d(11, 14, 0), d(10, 9, 30), d(9, 10, 0), d(9, 10, 20),
                ("Take the depot's zone from the DB, not the server's.", "Done: `depots.tz`."),
                None),
        MrSpec::new("statement-timeout", "perf(db): Statement timeout per query", &NADIA)
            .branch("perf/statement-timeout", "main")
            .milestone("v2.4.0")
            .labels(&["performance", "database"])
            .about("A slow report query held its connection for minutes. Queries get 5 s; the report pool 60 s.")
            .file(patch("src/db/pool.rs", &[(40, r#"
 pub async fn connect(settings: &DbSettings) -> Result<Db> {
     let options = PgConnectOptions::from_str(&settings.url)?
-        .application_name("orbit-api");
+        .application_name("orbit-api")
+        .options([("statement_timeout", "5s")]);
     let pool = PgPoolOptions::new()
         .max_connections(settings.max_connections)
-        .acquire_timeout(Duration::from_secs(30))
+        .acquire_timeout(Duration::from_secs(30))
+        .after_connect(|conn, _| Box::pin(async move { conn.execute("SET lock_timeout = '3s'").await.map(drop) }))
         .connect_with(options)
         .await?;
"#)]))
            .shipped(&[&THOMAS, &MARINA], d(9, 9, 0), d(8, 11, 0), d(6, 14, 0), d(6, 14, 30),
                ("5 s is too short for the reports: they scan a month.", "The reports get 60 s through their own pool."),
                Some("perf(db): Longer timeout for the report pool")),
        MrSpec::new("volumetric", "feat(quotes): Volumetric weight for oversized parcels", &SARAH)
            .branch("feat/4150-volumetric-weight", "main")
            .milestone("v2.4.0")
            .labels(&["feature", "pricing"])
            .about("Light but bulky parcels were priced on their weight alone.")
            .file(patch("src/pricing/quote.rs", &[(61, r#"
-    let weight = parcel.weight_kg;
+    let volumetric = parcel.volume_cm3() as f64 / carrier.volumetric_divisor as f64;
+    let weight = parcel.weight_kg.max(volumetric);
"#)]))
            .shipped(&[&MARINA, &ALEX], d(7, 10, 0), d(5, 15, 0), d(3, 11, 0), d(3, 14, 0),
                ("Divisor 5000 is the road one: air freight uses 6000.", "Per carrier now, from `carriers.volumetric_divisor`."),
                Some("feat(quotes): Volumetric divisor per carrier")),
        MrSpec::new("oauth-pkce", "feat(auth): OAuth2 PKCE flow for the mobile app", &ALEX)
            .branch("feat/4188-oauth-pkce", "main")
            .milestone("v2.4.0")
            .ticket(4188)
            .labels(&["feature", "security"])
            .about("The mobile app signs in with the authorization code flow and PKCE instead of a stored password.\n\nRefs #4188")
            .file(added("src/auth/pkce.rs", r#"
use sha2::{Digest, Sha256};

/// Checks a PKCE verifier against the challenge sent with the authorization request (S256 only).
pub fn verify(verifier: &str, challenge: &str, method: &str) -> Result<(), AuthError> {
    if method != "S256" {
        return Err(AuthError::InvalidRequest("only S256 challenges are accepted"));
    }
    let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (digest == challenge).then_some(()).ok_or(AuthError::InvalidGrant)
}
"#))
            .file(patch("src/auth/mod.rs", &[(3, r#"
 pub mod api_key;
+pub mod pkce;
 pub mod session;
"#)]))
            .shipped(&[&THOMAS, &MARINA], d(8, 9, 0), d(6, 10, 0), d(3, 16, 0), d(2, 10, 5),
                ("`plain` challenges must be refused: S256 only.", "Refused with `invalid_request`, tested."),
                Some("fix(auth): Refuse plain PKCE challenges")),
        MrSpec::new("csv-export", "feat(export): CSV export of shipments", &MARINA)
            .branch("feat/4310-csv-export", "main")
            .milestone("v2.4.0")
            .labels(&["feature"])
            .about("Account managers export a customer's shipments of a month for their reconciliation.")
            .file(added("src/export/csv.rs", r#"
/// One line per shipment, the columns the account managers asked for.
pub fn write(shipments: &[Shipment], out: impl Write) -> csv::Result<()> {
    let mut writer = csv::Writer::from_writer(out);
    writer.write_record(["number", "created", "status", "carrier", "weight_kg", "price"])?;
    for s in shipments {
        writer.serialize((&s.number, s.created_at, s.status, &s.carrier, s.weight_kg, s.price))?;
    }
    writer.flush().map_err(Into::into)
}
"#))
            .file(patch("src/api/mod.rs", &[(41, r#"
         .route("/shipments/:id", get(shipments::handler::detail))
+        .route("/shipments/export.csv", get(export::shipments_csv))
"#)]))
            .file(patch("dashboard/src/pages/Shipments.tsx", &[(88, r#"
       <Toolbar>
+        <Button href={`/api/shipments/export.csv?month=${month}`}>Export CSV</Button>
       </Toolbar>
"#)]))
            .at(d(6, 9, 30), Open)
            .at(d(6, 9, 31), Push(&["feat(export): CSV writer for shipments", "feat(export): GET /shipments/export.csv", "feat(export): Export button in the dashboard"], Ci::Pass))
            .at(d(6, 9, 35), Ask(&SARAH))
            .at(d(6, 9, 35), Ask(&ALEX))
            .at(d(5, 11, 0), Note(&SARAH, "bom", Some(("src/export/csv.rs", 3)), "Excel opens it as Latin-1: write a UTF-8 BOM first, or the accents break."))
            .at(d(4, 10, 0), Push(&["fix(export): UTF-8 BOM for Excel"], Ci::Flaky("integration-tests")))
            .at(d(4, 10, 5), Reply(&MARINA, "bom", "BOM written; tried in Excel and LibreOffice."))
            .at(d(3, 9, 30), Resolve(&SARAH, "bom"))
            .at(d(3, 9, 31), Approve(&SARAH))
            .at(d(2, 13, 50), Approve(&ALEX))
            .at(d(2, 14, 20), Merge(&ALEX))
            .status(S::NotOpen),
        MrSpec::new("migration-null", "fix(db): NULL tracking numbers broke migration 51", &NADIA)
            .branch("fix/migration-51-nulls", "main")
            .labels(&["bug", "database"])
            .about("Migration 51 adds `NOT NULL` to `tracking_number`; 312 shipments created by the old importer had none.")
            .file(patch("migrations/0051_tracking_number_not_null.sql", &[(1, r#"
+UPDATE shipments s SET tracking_number = l.tracking_number
+  FROM labels l WHERE l.shipment_id = s.id AND s.tracking_number IS NULL;
 ALTER TABLE shipments ALTER COLUMN tracking_number SET NOT NULL;
"#)]))
            .shipped(&[&THOMAS, &ALEX], d(2, 9, 10), d(2, 9, 40), d(2, 10, 50), d(2, 11, 30),
                ("Do we backfill the NULLs or skip them?", "Backfilled from the labels: 312 rows, all found."),
                None),
        MrSpec::new("hotfix-refresh", "fix(auth): Refresh tokens raced on expired sessions", &ALEX)
            .branch("hotfix/refresh-token-race", "main")
            .labels(&["bug", "hotfix"])
            .about("Two requests refreshing the same expired session both got a new token, and the mobile app was logged out. Picked to production at once.")
            .file(patch("src/auth/session.rs", &[(71, r#"
 pub async fn refresh(&self, session: SessionId) -> Result<Tokens> {
+    let lock = self.refresh_locks.entry(session).or_default().clone();
+    let _guard = lock.lock().await;
     let current = self.store.get(session).await?;
"#)]))
            .shipped(&[&THOMAS, &MARINA], d(1, 9, 5), d(1, 9, 30), d(1, 10, 50), d(1, 11, 10),
                ("Is the lock per session or global?", "Per session: a map of mutexes, cleaned on logout."),
                None),
        MrSpec::new("sccache-bucket", "ci: Cache sccache in the shared bucket", &SARAH)
            .branch("ci/sccache-bucket", "main")
            .labels(&["ci"])
            .about("The test jobs spent four minutes compiling dependencies on every pipeline.")
            .file(patch(".gitlab-ci.yml", &[(9, r#"
 variables:
   CARGO_HOME: $CI_PROJECT_DIR/.cargo
+  RUSTC_WRAPPER: sccache
+  SCCACHE_BUCKET: orbit-ci-sccache
+  SCCACHE_S3_USE_SSL: "true"
"#)]))
            .shipped(&[&ALEX, &MARINA], d(3, 9, 30), d(2, 15, 0), d(1, 14, 50), d(1, 15, 20),
                ("Is the bucket shared with the forks? They could poison the cache.", "Read-only for forks, written only from protected branches."),
                None),
        MrSpec::new("unknown-tracking", "fix(api): 404, not 500, for an unknown tracking number", &THOMAS)
            .branch("fix/unknown-tracking-number", "main")
            .labels(&["bug", "api"])
            .about("Partners calling with a mistyped number got a 500 and paged us at night.")
            .file(patch("src/api/public.rs", &[(5, r#"
-    let shipment = db.shipment_by_number(&number).await?.unwrap();
+    let shipment = db.shipment_by_number(&number).await?.ok_or(ApiError::NotFound)?;
"#)]))
            .shipped(&[&ALEX, &SARAH], d(1, 16, 0), d(1, 17, 10), m(40), m(9),
                ("Worth a test with a number that has the right format but does not exist.", "Added."),
                None),
    ]
}

/// The MRs open now, one for each state.
fn in_review() -> Vec<MrSpec> {
    vec![
        // Superseded by the GraphQL MR below, closed yesterday evening.
        MrSpec::new("juniper-prototype", "feat(api): GraphQL prototype with Juniper", &MARINA)
            .branch("spike/graphql-juniper", "main")
            .labels(&["spike"])
            .about("A first try at GraphQL for the partners, to see what the schema could look like.")
            .file(added("src/api/graphql_spike.rs", r#"
#[derive(GraphQLObject)]
struct Shipment {
    number: String,
    status: String,
}
"#))
            .at(d(31, 10, 0), Open)
            .at(d(31, 10, 1), Push(&["spike: GraphQL with Juniper"], Ci::Pass))
            .at(d(31, 10, 5), Ask(&THOMAS))
            .at(d(30, 15, 0), Note(&THOMAS, "lib", None, "async-graphql has first-class Axum support and a DataLoader: worth comparing before we go further?"))
            .at(d(30, 16, 20), Reply(&MARINA, "lib", "Agreed, I'll redo it with async-graphql."))
            .at(d(1, 18, 45), Close(&MARINA)),
        graphql(),
        MrSpec::new("pool-deadlock", "fix(db): Release connections when a booking times out", &THOMAS)
            .branch("fix/3987-pool-deadlock", "main")
            .milestone("v2.4.0")
            .ticket(3987)
            .assignee(&THOMAS)
            .labels(&["bug", "database", "priority::high"])
            .about(r#"
Under load, a booking that timed out kept its connection: after a few hundred the pool was empty and every request waited for 30 s.

## Fix

- The connection goes back to the pool when the booking future is dropped.
- The acquire timeout comes from the settings (`db.acquire_timeout`, 5 s by default).
- An index on `bookings (slot_id, status)` so the slot lookup stops scanning.

Refs #3987
"#)
            .file(patch("src/db/pool.rs", &[(38, r#"

 pub async fn connect(settings: &DbSettings) -> Result<Db> {
     let options = PgConnectOptions::from_str(&settings.url)?
         .application_name("orbit-api");
     let pool = PgPoolOptions::new()
         .max_connections(settings.max_connections)
-        .acquire_timeout(Duration::from_secs(30))
+        .acquire_timeout(settings.acquire_timeout)
+        .test_before_acquire(false)
         .connect_with(options)
         .await?;
"#), (88, r#"
 pub async fn book(db: &Db, slot: SlotId) -> Result<Booking> {
-    let mut tx = db.begin().await?;
+    let mut tx = scopeguard::guard(db.begin().await?, |tx| drop(tx));
     let booking = insert_booking(&mut tx, slot).await?;
"#)]))
            .file(patch("src/settings.rs", &[(52, r#"
     pub max_connections: u32,
+    /// How long a request waits for a connection before failing.
+    #[serde(with = "humantime_serde", default = "default_acquire_timeout")]
+    pub acquire_timeout: Duration,
 }
"#)]))
            .file(added("migrations/0049_bookings_slot_index.sql", r#"
CREATE INDEX bookings_slot_status ON bookings (slot_id, status);
"#))
            .at(d(13, 9, 30), Open)
            .at(d(13, 9, 31), Push(&["fix(db): Release connections when a booking times out"], Ci::Pass))
            .at(d(13, 9, 40), Ask(&SARAH))
            .at(d(13, 9, 40), Ask(&NADIA))
            .at(d(12, 11, 0), Note(&NADIA, "timeout", Some(("src/db/pool.rs", 46)), "A 30 s acquire timeout still holds the worker for 30 s under load. Can it come from the settings, with 5 s by default?"))
            .at(d(12, 14, 20), Push(&["fix(db): Acquire timeout from the settings", "perf(db): Index bookings on slot and status"], Ci::Flaky("integration-tests")))
            .at(d(12, 14, 25), Reply(&THOMAS, "timeout", "Now `db.acquire_timeout`, 5 s by default."))
            .at(d(11, 10, 10), Note(&SARAH, "index", Some(("migrations/0049_bookings_slot_index.sql", 1)), "This index is created without CONCURRENTLY: it will lock `bookings` on production during the deploy."))
            .at(d(11, 10, 30), Note(&SARAH, "metrics", None, "Could we expose the pool's wait time as a metric? It would have shown this deadlock weeks ago."))
            .at(d(10, 9, 0), Reply(&NADIA, "timeout", "5 s is fine for the API, but the report workers need more: keep them on their own setting?"))
            .status(S::Conflict)
            .flagged(),
        MrSpec::new("rate-limit", "feat(api): Token-bucket rate limiting per API key", &ALEX)
            .branch("feat/4315-rate-limit", "main")
            .milestone("v2.4.0")
            .ticket(4315)
            .assignee(&ALEX)
            .labels(&["feature", "api"])
            .about("Each API key gets a token bucket: 10 requests a second, bursts of 20. `X-RateLimit-*` headers on every response, 429 with `Retry-After` past the limit.\n\nRefs #4315")
            .file(added("src/auth/rate_limit.rs", r#"
use governor::{clock::DefaultClock, state::keyed::DashMapStateStore, Quota, RateLimiter};

pub type KeyedLimiter = RateLimiter<ApiKeyId, DashMapStateStore<ApiKeyId>, DefaultClock>;

/// 10 requests a second per key, bursts of 20 (per plan, from the settings).
pub fn limiter(plan: &PlanLimits) -> KeyedLimiter {
    let quota = Quota::per_second(plan.per_second).allow_burst(plan.burst);
    RateLimiter::dashmap(quota)
}

pub async fn enforce(State(limiter): State<Arc<KeyedLimiter>>, key: ApiKey, req: Request, next: Next) -> Response {
    match limiter.check_key(&key.id) {
        Ok(()) => next.run(req).await,
        Err(not_until) => too_many_requests(not_until.wait_time_from(DefaultClock::default().now())),
    }
}
"#))
            .file(patch("src/auth/api_key.rs", &[(12, r#"
 pub struct ApiKey {
     pub id: ApiKeyId,
     pub account: AccountId,
+    pub plan: Plan,
 }
"#)]))
            .file(patch("src/api/mod.rs", &[(62, r#"
-    router.layer(middleware::from_fn_with_state(state.clone(), auth::authenticate))
+    router
+        .layer(middleware::from_fn_with_state(state.limiter.clone(), auth::rate_limit::enforce))
+        .layer(middleware::from_fn_with_state(state.clone(), auth::authenticate))
"#)]))
            .at(d(7, 10, 0), Open)
            .at(d(7, 10, 1), Push(&["feat(api): Token bucket per API key", "feat(api): Rate-limit headers on every response"], Ci::Pass))
            .at(d(7, 10, 5), Ask(&THOMAS))
            .at(d(7, 10, 5), Ask(&SARAH))
            .at(d(6, 15, 0), Note(&THOMAS, "burst", Some(("src/auth/rate_limit.rs", 7)), "A burst of 100 lets a script empty a day's quota in a second. 20?"))
            .at(d(5, 9, 30), Push(&["fix(api): Bursts of 20 requests"], Ci::Pass))
            .at(d(5, 9, 32), Reply(&ALEX, "burst", "20, and configurable per plan."))
            .at(d(5, 11, 0), Resolve(&THOMAS, "burst"))
            .at(d(5, 11, 1), Approve(&THOMAS))
            .at(d(4, 10, 30), Approve(&SARAH))
            .status(S::Mergeable),
        MrSpec::new("status-index", "perf(db): Index shipments on (status, updated_at)", &THOMAS)
            .branch("perf/shipments-status-index", "main")
            .milestone("v2.4.0")
            .labels(&["performance", "database"])
            .about("The partners' \"what changed since\" polling scans `shipments`: 1.8 s at p95. With the index: 12 ms.")
            .file(added("migrations/0053_shipments_status_index.sql", r#"
-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS shipments_status_updated ON shipments (status, updated_at);
"#))
            .file(patch("src/db/shipments.rs", &[(140, r#"
 /// The shipments of `account` changed since `since`, oldest change first.
-pub async fn changed_since(db: &Db, account: AccountId, since: DateTime<Utc>) -> Result<Vec<Shipment>> {
+/// Served by `shipments_status_updated`.
+pub async fn changed_since(db: &Db, account: AccountId, since: DateTime<Utc>) -> Result<Vec<Shipment>> {
"#)]))
            .at(d(4, 14, 0), Open)
            .at(d(4, 14, 1), Push(&["perf(db): Index shipments on (status, updated_at)"], Ci::Pass))
            .at(d(4, 14, 5), Ask(&NADIA))
            .at(d(4, 14, 5), Ask(&MARINA))
            .at(d(3, 10, 0), Note(&NADIA, "concurrently", Some(("migrations/0053_shipments_status_index.sql", 2)), "CONCURRENTLY cannot run inside a transaction: mark the migration `-- no-transaction`."))
            .at(d(3, 11, 30), Push(&["fix(db): Run the index migration outside a transaction"], Ci::Pass))
            .at(d(3, 11, 31), Reply(&THOMAS, "concurrently", "Done."))
            .at(d(3, 13, 0), Resolve(&NADIA, "concurrently"))
            .at(d(3, 13, 1), Approve(&NADIA))
            .at(d(2, 9, 15), Approve(&MARINA))
            .at(d(2, 11, 40), Approve(&ALEX))
            .status(S::Mergeable),
        MrSpec::new("webhook-docs", "docs(api): Document the webhook retry policy", &THOMAS)
            .branch("docs/4350-webhook-retries", "main")
            .ticket(4350)
            .labels(&["documentation"])
            .about("Partners keep asking how often a failed webhook is retried.\n\nRefs #4350")
            .file(patch("docs/webhooks.md", &[(40, r#"
 ## Delivery

+### Retries
+
+A delivery that fails (no answer within 10 s, or a 5xx) is tried again after
+1, 2, 4, 8, 16, 32 and 60 minutes: 8 attempts over about two hours. Each
+attempt carries the same `Orbit-Delivery-Id`, so you can drop duplicates.
+
"#)]))
            .at(d(1, 15, 40), Open)
            .at(d(1, 15, 41), Push(&["docs(api): Webhook retry policy"], Ci::Pass)),
        push_notifications(),
        MrSpec::new("notif-preferences", "feat(notif): Notification preferences per customer", &JULIEN)
            .branch("feat/4302-notification-preferences", "feat/4302-push-notifications")
            .milestone("v2.5.0")
            .ticket(4302)
            .labels(&["feature"])
            .about("Customers choose which statuses notify them. Stacked on the push notifications MR.\n\nRefs #4302")
            .file(added("migrations/0054_notification_preferences.sql", r#"
CREATE TABLE notification_preferences (
    customer_id uuid PRIMARY KEY REFERENCES customers (id),
    statuses text[] NOT NULL DEFAULT '{out_for_delivery,delivered}'
);
"#))
            .file(added("src/api/preferences.rs", r#"
pub async fn update(State(db): State<Db>, customer: Customer, Json(body): Json<Preferences>) -> ApiResult<StatusCode> {
    db.save_preferences(customer.id, &body.statuses).await?;
    Ok(StatusCode::NO_CONTENT)
}
"#))
            .at(d(6, 14, 0), Open)
            .at(d(6, 14, 1), Push(&["feat(notif): Preferences table and endpoint"], Ci::Pass))
            .at(d(6, 14, 5), Ask(&THOMAS))
            .at(d(6, 14, 5), Ask(&MARINA))
            .at(d(6, 14, 5), Ask(&NADIA))
            .at(d(5, 10, 0), Approve(&MARINA))
            .at(d(4, 16, 0), Approve(&THOMAS))
            .at(d(4, 16, 30), Approve(&NADIA))
            .status(S::Mergeable),
        MrSpec::new("notif-mute", "Draft: feat(notif): Mute notifications per shipment", &JULIEN)
            .branch("feat/4302-mute-notifications", "feat/4302-notification-preferences")
            .milestone("v2.5.0")
            .ticket(4302)
            .labels(&["feature"])
            .about("Work in progress: a mute switch on the tracking page.")
            .file(patch("src/api/preferences.rs", &[(5, r#"
+
+pub async fn mute(State(db): State<Db>, customer: Customer, Path(shipment): Path<ShipmentId>) -> ApiResult<StatusCode> {
+    db.mute(customer.id, shipment).await?;
+    Ok(StatusCode::NO_CONTENT)
+}
"#)]))
            .at(d(3, 11, 0), Open)
            .at(d(3, 11, 1), Push(&["feat(notif): Mute flag on shipments"], Ci::Pass))
            .at(d(2, 17, 0), Push(&["wip: mute from the tracking page"], Ci::Pass))
            .status(S::Draft),
        MrSpec::new("pkce-verifier", "feat(auth): Keep the PKCE verifier across app restarts", &SARAH)
            .branch("feat/4188-pkce-verifier-store", "feat/4188-oauth-pkce")
            .milestone("v2.4.0")
            .ticket(4188)
            .labels(&["feature", "security"])
            .about("Android kills the app during the login redirect: the verifier is kept server-side for 10 minutes, keyed by `state`.\n\nRefs #4188")
            .file(patch("src/auth/pkce.rs", &[(11, r#"
+
+/// Keeps a verifier for 10 minutes, keyed by the authorization request's `state`.
+pub async fn remember(cache: &Cache, state: &str, verifier: &str) -> Result<()> {
+    cache.set_ex(format!("pkce:{state}"), verifier, 600).await
+}
"#)]))
            .at(d(4, 11, 0), Open)
            .at(d(4, 11, 1), Push(&["feat(auth): Keep the verifier for 10 minutes"], Ci::Pass))
            .at(d(4, 11, 5), Ask(&THOMAS))
            .at(d(4, 11, 5), Ask(&ALEX))
            .at(d(3, 9, 0), Approve(&ALEX)),
        tokio_update(),
        courier_websocket(),
        MrSpec::new("vat-per-line", "fix(billing): Round VAT per line, not per invoice", &SARAH)
            .branch("fix/4277-vat-per-line", "main")
            .milestone("v2.5.0")
            .ticket(4277)
            .labels(&["bug", "billing"])
            .about("Invoices were off by a cent against the accounting export, which rounds each line.\n\nRefs #4277")
            .file(patch("src/billing/vat.rs", &[(22, r#"
-pub fn vat(invoice: &Invoice) -> Money {
-    (invoice.net() * invoice.rate).round_dp(2)
+pub fn vat(invoice: &Invoice) -> Money {
+    invoice
+        .lines
+        .iter()
+        .map(|line| (line.net * invoice.rate).round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero))
+        .sum()
 }
"#)]))
            .at(d(3, 15, 0), Open)
            .at(d(3, 15, 1), Push(&["fix(billing): VAT rounded per line"], Ci::Pass))
            .at(d(3, 15, 5), Ask(&MARINA))
            .at(d(2, 10, 0), Note(&MARINA, "rounding", Some(("src/billing/vat.rs", 27)), "Banker's rounding or half-up? The accounting export uses half-up."))
            .at(d(2, 10, 58), Push(&["fix(billing): Half-up rounding like the accounting export"], Ci::Canceled))
            .at(d(2, 11, 0), Reply(&SARAH, "rounding", "Half-up like the export, with a test on 0.005. I cancelled the pipeline: runners were saturated, I'll run it again tonight."))
            .at(d(2, 15, 0), Resolve(&MARINA, "rounding"))
            .at(d(2, 15, 1), Approve(&MARINA))
            .status(S::CiMustPass),
        MrSpec::new("revoke-keys", "feat(auth): Revoke API keys from the dashboard", &ALEX)
            .branch("feat/4290-revoke-api-keys", "main")
            .milestone("Security audit")
            .ticket(4290)
            .labels(&["feature", "security"])
            .about("Finding #7 of the audit: a leaked key could only be revoked by support.\n\nRefs #4290")
            .file(patch("src/auth/api_key.rs", &[(80, r#"
+/// Revokes `key` at once: removed from the store and from every instance's cache.
+pub async fn revoke(ctx: &Ctx, by: UserId, key: ApiKeyId) -> Result<()> {
+    ctx.db.revoke_api_key(key).await?;
+    ctx.bus.publish(Event::ApiKeyRevoked(key)).await?;
+    ctx.audit.record(by, AuditAction::RevokeApiKey { prefix: key.prefix() }).await
+}
"#)]))
            .file(patch("dashboard/src/pages/ApiKeys.tsx", &[(41, r#"
         <td>{key.lastUsed ?? 'never'}</td>
+        <td><Button danger onClick={() => revoke(key.id)}>Revoke</Button></td>
"#)]))
            .at(d(9, 10, 0), Open)
            .at(d(9, 10, 1), Push(&["feat(auth): Revoke an API key", "feat(dashboard): Revoke button"], Ci::Pass))
            .at(d(9, 10, 5), Ask(&THOMAS))
            .at(d(9, 10, 5), Ask(&MARINA))
            .at(d(7, 14, 0), Note(&THOMAS, "cache", Some(("src/auth/api_key.rs", 83)), "Revoked keys stay valid for 5 minutes in the auth cache: evict them on revoke."))
            .at(d(7, 14, 10), Note(&THOMAS, "audit", None, "The revocation should land in the audit log, with who did it."))
            .at(d(6, 11, 0), Push(&["fix(auth): Evict revoked keys from every cache", "feat(auth): Audit log entry on revoke"], Ci::Pass))
            .at(d(6, 11, 5), Reply(&ALEX, "cache", "Evicted on revoke, on every instance through the bus."))
            .at(d(6, 11, 6), Reply(&ALEX, "audit", "Logged with the user and the key's prefix."))
            .at(d(5, 9, 0), Approve(&THOMAS))
            .at(d(5, 9, 30), Approve(&MARINA))
            .status(S::DiscussionsNotResolved),
        MrSpec::new("split-handlers", "refactor(api): One module per shipments route", &MARINA)
            .branch("refactor/shipments-handlers", "main")
            .milestone("Tech debt")
            .labels(&["refactoring"])
            .about("`handler.rs` is 1 400 lines. Each route moves to its own module; no behaviour change.")
            .file(FileChange {
                old_path: Some("src/api/shipments/handler.rs".into()),
                ..patch("src/api/shipments/list.rs", &[(1, r#"
-//! The shipments routes.
+//! `GET /shipments`: the list, filtered and paginated.
 use crate::db::shipments::{self, ShipmentFilter};
"#)])
            })
            .file(added("src/api/shipments/mod.rs", r#"
mod create;
mod detail;
mod list;

pub use create::create;
pub use detail::detail;
pub use list::list;
"#))
            .file(patch("src/api/mod.rs", &[(40, r#"
-        .route("/shipments", get(shipments::handler::list).post(shipments::handler::create))
-        .route("/shipments/:id", get(shipments::handler::detail))
+        .route("/shipments", get(shipments::list).post(shipments::create))
+        .route("/shipments/:id", get(shipments::detail))
"#)]))
            .at(d(2, 16, 0), Open)
            .at(d(2, 16, 1), Push(&["refactor(api): One module per shipments route"], Ci::Pass))
            .at(d(2, 16, 5), Ask(&JULIEN))
            .at(h(3), Rebase(Ci::Pass))
            .status(S::SyncFailed),
        MrSpec::new("livez-readyz", "chore(ops): Split the health check into /livez and /readyz", &NADIA)
            .branch("chore/livez-readyz", "main")
            .milestone("Tech debt")
            .labels(&["ops"])
            .about("Kubernetes restarted pods when Postgres was slow: liveness must not depend on the database.")
            .file(patch("src/api/health.rs", &[(1, r#"
-pub async fn health(State(db): State<Db>) -> StatusCode {
-    if db.ping().await.is_ok() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }
+/// The process answers: Kubernetes keeps the pod.
+pub async fn livez() -> StatusCode {
+    StatusCode::OK
+}
+
+/// Ready for traffic: the database answers.
+pub async fn readyz(State(db): State<Db>) -> StatusCode {
+    if db.ping().await.is_ok() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }
 }
"#)]))
            .file(patch("chart/templates/deployment.yaml", &[(44, r#"
           livenessProbe:
-            httpGet: { path: /health, port: http }
+            httpGet: { path: /livez, port: http }
           readinessProbe:
-            httpGet: { path: /health, port: http }
+            httpGet: { path: /readyz, port: http }
"#)]))
            .at(d(4, 10, 0), Open)
            .at(d(4, 10, 1), Push(&["chore(ops): /livez and /readyz"], Ci::Pass))
            .at(d(4, 10, 5), Ask(&SARAH))
            .at(d(4, 10, 5), Ask(&ALEX))
            .at(d(3, 15, 30), Approve(&SARAH))
            .at(d(3, 16, 0), Approve(&ALEX))
            // The security scan's external status check has not answered.
            .status(S::Unknown),
        MrSpec::new("zpl-labels", "feat(labels): Print shipping labels as ZPL", &SARAH)
            .branch("feat/4320-zpl-labels", "main")
            .milestone("v2.4.0")
            .ticket(4320)
            .labels(&["feature"])
            .about("Depots print on Zebra printers: ZPL instead of PDF saves the conversion on the depot PCs.\n\nRefs #4320")
            .file(added("src/labels/zpl.rs", r#"
/// The label as ZPL for a printer of `dpi` dots per inch.
pub fn render(label: &Label, dpi: u32) -> String {
    let dots = |mm: f32| (mm / 25.4 * dpi as f32).round() as u32;
    let mut zpl = String::from("^XA");
    zpl += &format!("^FO{},{}^BCN,{},Y,N,N^FD{}^FS", dots(5.0), dots(5.0), dots(20.0), label.tracking_number);
    zpl += &format!("^FO{},{}^A0N,{},{}^FD{}^FS", dots(5.0), dots(30.0), dots(4.0), dots(4.0), label.recipient_city);
    zpl + "^XZ"
}
"#))
            .at(d(5, 14, 0), Open)
            .at(d(5, 14, 1), Push(&["feat(labels): ZPL renderer"], Ci::Pass))
            .at(d(5, 14, 5), Ask(&ALEX))
            .at(d(5, 14, 5), Ask(&JULIEN))
            .at(d(4, 9, 0), Note(&ALEX, "dpi", Some(("src/labels/zpl.rs", 3)), "203 dpi is hard-coded upstream: the Zebra ZT411 at the Lyon depot prints at 300."))
            .at(d(3, 9, 0), Push(&["feat(labels): Printer resolution per depot"], Ci::Pass))
            .at(d(3, 9, 2), Reply(&SARAH, "dpi", "Now from `depots.printer_dpi`."))
            .at(d(3, 9, 40), Resolve(&ALEX, "dpi"))
            .at(d(3, 9, 41), Approve(&ALEX))
            // The last refresh of this MR failed: what was read before stays.
            .fetch(Fetch::Failed("GitLab API returned 502 Bad Gateway")),
    ]
}

fn graphql() -> MrSpec {
    MrSpec::new("graphql-shipments", "feat(api): GraphQL endpoint for shipments", &MARINA)
        .branch("feat/4271-graphql-shipments", "main")
        .milestone("v2.4.0")
        .ticket(4271)
        .assignee(&THOMAS)
        .labels(&["feature", "api", "partners"])
        .flagged()
        .about(r#"
Partners asked for one query giving a page of shipments with their scans, instead of one REST call per shipment. This MR adds `/graphql`, built with **async-graphql** behind the existing API-key authentication.

## Motivation

The three largest partners poll `/v2/shipments/{id}` for every open parcel: 2.1 M calls a day, 70 % of our API traffic. Each answer is 4 KB when they read five fields.

## What changes

- `Query.shipments(first, after, status)`: a page of shipments, cursor-paginated like the REST list, at most 100 at a time.
- `Query.shipment(number)`: one shipment by its tracking number.
- `Shipment.scans`: loaded for the whole page at once with a DataLoader (2 queries whatever the page size, instead of 1 + n).
- Depth limit 6 and complexity limit 500, so a single query cannot walk the whole graph.
- Introspection off in production.

## Schema

The schema is snapshotted in `tests/snapshots/schema.graphql`: a change to it shows in the review.

## Not in this MR

- Subscriptions (live scans) come with the courier WebSocket work, in v2.5.0.
- Mutations: partners keep creating shipments through REST for now.

## Testing

- 31 new tests, among them the N+1 check: a page of 50 shipments runs 2 queries.
- Load test on staging: 400 queries a second, p99 at 38 ms.

Refs #4271
"#)
        .file(added("src/api/graphql/mod.rs", r#"
mod loaders;
mod pagination;
mod schema;

pub use schema::{build_schema, OrbitSchema};

/// `POST /graphql`, behind the API-key authentication.
pub async fn handler(State(schema): State<OrbitSchema>, key: ApiKey, req: GraphQLRequest) -> GraphQLResponse {
    schema.execute(req.into_inner().data(key)).await.into()
}
"#))
        .file(added("src/api/graphql/schema.rs", r#"
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Result, Schema};

pub type OrbitSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// A page of the caller's shipments, newest first.
    async fn shipments(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
        status: Option<ShipmentStatus>,
    ) -> Result<Connection<String, Shipment>> {
        let key = ctx.data::<ApiKey>()?;
        let page = Page::new(first, after)?;
        let rows = shipments::list(ctx.data::<Db>()?, key.account, status, &page).await?;
        Ok(page.connection(rows))
    }

    /// One shipment by its tracking number.
    async fn shipment(&self, ctx: &Context<'_>, number: String) -> Result<Option<Shipment>> {
        let key = ctx.data::<ApiKey>()?;
        Ok(shipments::by_number(ctx.data::<Db>()?, key.account, &number).await?)
    }
}

#[Object]
impl Shipment {
    async fn number(&self) -> &str {
        &self.number
    }

    async fn status(&self) -> ShipmentStatus {
        self.status
    }

    /// The scans of the shipment, oldest first, loaded for the whole page at once.
    async fn scans(&self, ctx: &Context<'_>) -> Result<Vec<Scan>> {
        let loader = ctx.data::<DataLoader<ScanLoader>>()?;
        Ok(loader.load_one(self.id).await?.unwrap_or_default())
    }
}

pub fn build_schema(db: Db) -> OrbitSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(DataLoader::new(ScanLoader::new(db.clone()), tokio::spawn))
        .data(db)
        .limit_depth(6)
        .limit_complexity(500)
        .disable_introspection()
        .finish()
}
"#))
        .file(added("src/api/graphql/loaders.rs", r#"
use async_graphql::dataloader::Loader;
use std::collections::HashMap;

/// The scans of many shipments in one query.
pub struct ScanLoader {
    db: Db,
}

impl Loader<ShipmentId> for ScanLoader {
    type Value = Vec<Scan>;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, ids: &[ShipmentId]) -> Result<HashMap<ShipmentId, Vec<Scan>>, Self::Error> {
        let scans = scans::for_shipments(&self.db, ids).await.map_err(Arc::new)?;
        Ok(scans.into_iter().fold(HashMap::new(), |mut by_id, scan| {
            by_id.entry(scan.shipment_id).or_insert_with(Vec::new).push(scan);
            by_id
        }))
    }
}
"#))
        .file(added("src/api/graphql/pagination.rs", r#"
/// At most this many shipments a page, as the REST list.
pub const MAX_PAGE: i32 = 100;

pub struct Page {
    pub limit: i64,
    pub after: Option<Cursor>,
}

impl Page {
    pub fn new(first: Option<i32>, after: Option<String>) -> Result<Self> {
        let limit = first.unwrap_or(20).clamp(1, MAX_PAGE) as i64;
        let after = after.map(|c| Cursor::decode(&c)).transpose()?;
        Ok(Self { limit, after })
    }
}
"#))
        .file(patch("src/api/mod.rs", &[(38, r#"
 pub fn router(state: AppState) -> Router {
     Router::new()
+        .route("/graphql", post(graphql::handler))
         .route("/shipments", get(shipments::handler::list).post(shipments::handler::create))
"#)]))
        .file(patch("Cargo.toml", &[(14, r#"
 [dependencies]
+async-graphql = { version = "7.0", features = ["dataloader", "chrono"] }
+async-graphql-axum = "7.0"
 axum = { version = "0.7", features = ["macros"] }
"#)]))
        .file(schema_snapshot())
        .at(d(9, 9, 30), Open)
        .at(d(9, 9, 31), Push(&["feat(api): GraphQL schema for shipments", "feat(api): Shipment and Scan resolvers"], Ci::Pass))
        .at(d(9, 9, 40), Ask(&THOMAS))
        .at(d(9, 9, 40), Ask(&ALEX))
        .at(d(8, 11, 20), Note(&ALEX, "n+1", Some(("src/api/graphql/schema.rs", 45)), "The resolver loads the scans one shipment at a time: a page of 50 makes 51 queries. A DataLoader would batch them."))
        .at(d(8, 14, 5), Reply(&MARINA, "n+1", "Good catch: a DataLoader keyed by shipment id."))
        .at(d(7, 10, 15), Push(&["perf(api): Batch scan loading with a DataLoader"], Ci::Flaky("api-contract")))
        .at(d(7, 10, 20), Reply(&MARINA, "n+1", "Done: 2 queries whatever the page size, with a test counting them."))
        .at(d(6, 9, 50), Resolve(&ALEX, "n+1"))
        .at(d(6, 9, 52), Approve(&ALEX))
        .at(d(3, 16, 10), Push(&["feat(api): Cursor pagination on shipments"], Ci::Pass))
        .at(d(2, 9, 0), Note(&ALEX, "cap", Some(("src/api/graphql/pagination.rs", 2)), "Should `first` be capped? A client asking for 10 000 shipments gets them all."))
        .at(d(2, 11, 28), Push(&["fix(api): Cap a page at 100 shipments"], Ci::Pass))
        .at(d(2, 11, 30), Reply(&MARINA, "cap", "Capped at 100 like the REST list, in the last push."))
        .at(m(21), Reply(&ALEX, "cap", "Thanks, resolving."))
        .at(m(20), Resolve(&ALEX, "cap"))
        .failure("api-contract", "Schemathesis v3.36.2\nCollected API operations: 39\n\n...........................E\n\n================================ ERRORS ================================\n_____________________________ POST /graphql _____________________________\nrequests.exceptions.ReadTimeout: HTTPConnectionPool(host='localhost', port=8080): Read timed out. (read timeout=10)\n\nThe API was still warming its connection pool: run the job again.\n=================== 38 passed, 1 errored in 101.44s ===================\nERROR: Job failed: exit code 1")
}

/// The schema snapshot the GraphQL MR adds, as `async-graphql` prints it.
fn schema_snapshot() -> FileChange {
    const TYPES: [(&str, &str, &[(&str, &str, &str)]); 9] = [
        ("type", "Query", &[
            ("shipments", "(first: Int, after: String, status: ShipmentStatus): ShipmentConnection!", "A page of the caller's shipments, newest first."),
            ("shipment", "(number: String!): Shipment", "One shipment by its tracking number."),
        ]),
        ("type", "Shipment", &[
            ("id", ": ID!", ""),
            ("number", ": String!", "The tracking number printed on the label."),
            ("status", ": ShipmentStatus!", ""),
            ("createdAt", ": DateTime!", ""),
            ("carrier", ": Carrier!", ""),
            ("weightKg", ": Float!", "The weight used for the price: the real one, or the volumetric one when larger."),
            ("eta", ": Eta", "When the parcel should arrive; null until it is picked up."),
            ("scans", ": [Scan!]!", "The scans of the shipment, oldest first."),
            ("recipient", ": Recipient!", ""),
        ]),
        ("type", "Scan", &[
            ("id", ": ID!", ""),
            ("scannedAt", ": DateTime!", ""),
            ("kind", ": ScanKind!", ""),
            ("depot", ": Depot", "The depot where it was scanned; null for a courier's scan."),
            ("position", ": GeoPoint", ""),
        ]),
        ("type", "Carrier", &[
            ("code", ": String!", ""),
            ("name", ": String!", ""),
            ("trackingUrl", ": String", ""),
        ]),
        ("type", "Depot", &[
            ("code", ": String!", ""),
            ("city", ": String!", ""),
            ("timeZone", ": String!", ""),
        ]),
        ("type", "Eta", &[
            ("date", ": Date!", ""),
            ("from", ": Time", "The start of the delivery window, when known."),
            ("to", ": Time", ""),
        ]),
        ("type", "Recipient", &[
            ("city", ": String!", ""),
            ("postcode", ": String!", ""),
            ("country", ": String!", "ISO 3166-1 alpha-2."),
        ]),
        ("type", "ShipmentConnection", &[
            ("edges", ": [ShipmentEdge!]!", ""),
            ("pageInfo", ": PageInfo!", ""),
        ]),
        ("type", "PageInfo", &[
            ("hasNextPage", ": Boolean!", ""),
            ("endCursor", ": String", ""),
        ]),
    ];
    let mut text = String::new();
    for (kind, name, fields) in TYPES {
        text.push_str(&format!("{kind} {name} {{\n"));
        for (field, ty, doc) in fields.iter() {
            if !doc.is_empty() {
                text.push_str(&format!("\t\"\"\"\n\t{doc}\n\t\"\"\"\n"));
            }
            text.push_str(&format!("\t{field}{ty}\n"));
        }
        text.push_str("}\n\n");
    }
    text.push_str("type ShipmentEdge {\n\tnode: Shipment!\n\tcursor: String!\n}\n\n");
    for (name, values) in [
        (
            "ShipmentStatus",
            "CREATED PICKED_UP IN_TRANSIT OUT_FOR_DELIVERY DELIVERED RETURNED",
        ),
        (
            "ScanKind",
            "PICKUP DEPOT_IN DEPOT_OUT COURIER DELIVERY FAILED_ATTEMPT",
        ),
    ] {
        text.push_str(&format!("enum {name} {{\n"));
        for value in values.split(' ') {
            text.push_str(&format!("\t{value}\n"));
        }
        text.push_str("}\n\n");
    }
    text.push_str("type GeoPoint {\n\tlat: Float!\n\tlon: Float!\n}\n\nscalar Date\n\nscalar DateTime\n\nscalar Time\n\nschema {\n\tquery: Query\n}\n");
    added("tests/snapshots/schema.graphql", &text)
}

fn push_notifications() -> MrSpec {
    MrSpec::new("push-notifications", "feat(notif): Push notifications on delivery status change", &JULIEN)
        .branch("feat/4302-push-notifications", "main")
        .milestone("v2.5.0")
        .ticket(4302)
        .labels(&["feature", "mobile"])
        .about(r#"
Recipients who installed the app get a push when their parcel changes status: picked up, out for delivery, delivered, delivery failed.

- FCM for Android, APNs for iOS, behind one `PushSender` trait.
- Only a real change notifies: a parcel scanned twice at a depot does not.
- Device tokens are stored per customer and pruned when the stores reject them.

Refs #4302
"#)
        .file(added("src/notify/push.rs", r#"
/// Sends a push for each status change of a shipment, once.
pub struct StatusNotifier<S: PushSender> {
    sender: S,
    last: LastNotified,
}

impl<S: PushSender> StatusNotifier<S> {
    pub async fn on_scan(&self, shipment: &Shipment, scan: &Scan) -> Result<()> {
        let status = shipment.status_after(scan);
        if self.last.get(shipment.id).await? == Some(status) {
            return Ok(());
        }
        for device in self.devices(shipment.recipient).await? {
            self.sender.send(&device, Message::status(shipment, status)).await?;
        }
        self.last.set(shipment.id, status).await
    }
}
"#))
        .file(added("src/notify/fcm.rs", r#"
pub struct Fcm {
    client: reqwest::Client,
    project: String,
}

impl PushSender for Fcm {
    async fn send(&self, device: &Device, message: Message) -> Result<()> {
        tracing::debug!(device = %device.id, "sending push");
        self.client.post(self.url()).json(&message.for_fcm(&device.token)).send().await?.error_for_status()?;
        Ok(())
    }
}
"#))
        .file(patch("src/events/bus.rs", &[(28, r#"
 pub enum Event {
     ShipmentCreated(ShipmentId),
     Scanned(ShipmentId, ScanId),
+    StatusChanged(ShipmentId, ShipmentStatus),
     ApiKeyRevoked(ApiKeyId),
 }
"#)]))
        .at(d(10, 9, 0), Open)
        .at(d(10, 9, 1), Push(&["feat(notif): FCM and APNs clients", "feat(notif): Notify on status change"], Ci::Pass))
        .at(d(10, 9, 5), Ask(&THOMAS))
        .at(d(10, 9, 5), Ask(&MARINA))
        .at(d(8, 15, 0), Note(&THOMAS, "twice", Some(("src/notify/push.rs", 10)), "A parcel scanned twice at the depot sends two pushes: its status did not change the second time."))
        .at(d(8, 15, 5), Changes(&THOMAS))
        .at(d(7, 11, 0), Push(&["fix(notif): Notify on status changes only"], Ci::Fail(&["unit-tests"])))
        .at(d(6, 10, 0), Push(&["test(notif): Cover the double scan"], Ci::Pass))
        .at(d(5, 16, 0), Reply(&JULIEN, "twice", "Fixed: we compare with the last status notified."))
        .at(d(4, 9, 30), Note(&MARINA, "token", Some(("src/notify/fcm.rs", 9)), "The FCM token ends up in the logs at DEBUG: it is a credential, keep it out."))
        .at(h(5), Push(&["fix(notif): Keep device tokens out of the logs", "refactor(notif): Last notified status in Redis"], Ci::Fail(&["unit-tests"])))
        .status(S::RequestedChanges)
        .failure("unit-tests", "    Starting 418 tests across 6 binaries\n        PASS [   0.012s] orbit-core shipment::tests::status_follows_the_scans\n        FAIL [   0.044s] orbit-api notify::push::tests::one_push_per_status_change\n\n--- STDOUT: orbit-api notify::push::tests::one_push_per_status_change ---\nthread 'notify::push::tests::one_push_per_status_change' panicked at src/notify/push.rs:61:9:\nassertion `left == right` failed: a parcel scanned twice at the depot pushes twice\n  left: 2\n right: 1\nnote: the in-memory LastNotified of the test is not shared with the Redis one\n------------\n     Summary [  39.020s] 418 tests run: 417 passed, 1 failed, 3 skipped\nerror: test run failed\nERROR: Job failed: exit code 100")
}

fn tokio_update() -> MrSpec {
    let bumps: [(&str, &str, &str); 30] = [
        ("async-trait", "0.1.83", "0.1.84"),
        ("axum", "0.7.7", "0.7.9"),
        ("bytes", "1.7.2", "1.9.0"),
        ("futures", "0.3.30", "0.3.31"),
        ("futures-channel", "0.3.30", "0.3.31"),
        ("futures-core", "0.3.30", "0.3.31"),
        ("futures-executor", "0.3.30", "0.3.31"),
        ("futures-io", "0.3.30", "0.3.31"),
        ("futures-macro", "0.3.30", "0.3.31"),
        ("futures-sink", "0.3.30", "0.3.31"),
        ("futures-task", "0.3.30", "0.3.31"),
        ("futures-util", "0.3.30", "0.3.31"),
        ("h2", "0.4.6", "0.4.7"),
        ("hyper", "1.4.1", "1.5.1"),
        ("hyper-util", "0.1.9", "0.1.10"),
        ("libc", "0.2.159", "0.2.167"),
        ("mio", "1.0.2", "1.0.3"),
        ("pin-project-lite", "0.2.14", "0.2.15"),
        ("socket2", "0.5.7", "0.5.8"),
        ("tokio", "1.40.0", "1.41.1"),
        ("tokio-macros", "2.4.0", "2.5.0"),
        ("tokio-rustls", "0.26.0", "0.26.1"),
        ("tokio-stream", "0.1.16", "0.1.17"),
        ("tokio-util", "0.7.12", "0.7.13"),
        ("tower", "0.5.1", "0.5.2"),
        ("tower-http", "0.6.1", "0.6.2"),
        ("tracing", "0.1.40", "0.1.41"),
        ("tracing-attributes", "0.1.27", "0.1.28"),
        ("tracing-core", "0.1.32", "0.1.33"),
        ("tracing-subscriber", "0.3.18", "0.3.19"),
    ];
    MrSpec::new("tokio-ecosystem", "chore(deps): Update the tokio ecosystem", &RENOVATE)
        .branch("renovate/tokio-ecosystem", "main")
        .assignee(&THOMAS)
        .labels(&["dependencies"])
        .about("This PR contains the following updates:\n\n| Package | Change |\n|---|---|\n| tokio | 1.40.0 → 1.41.1 |\n| hyper | 1.4.1 → 1.5.1 |\n| axum | 0.7.7 → 0.7.9 |\n| tower-http | 0.6.1 → 0.6.2 |\n\nand 26 more in `Cargo.lock`.\n\n🔕 Ignore: close this MR and you won't be reminded about these updates again.")
        .file(patch("Cargo.toml", &[(16, r#"
-axum = { version = "0.7.7", features = ["macros"] }
+axum = { version = "0.7.9", features = ["macros"] }
 serde = { version = "1", features = ["derive"] }
-tokio = { version = "1.40", features = ["full"] }
-tower-http = { version = "0.6.1", features = ["trace", "cors", "compression-gzip"] }
+tokio = { version = "1.41", features = ["full"] }
+tower-http = { version = "0.6.2", features = ["trace", "cors", "compression-gzip"] }
"#)]))
        .file(cargo_lock(118, &bumps))
        .at(d(4, 5, 12), Open)
        .at(d(4, 5, 13), Push(&["chore(deps): Update the tokio ecosystem"], Ci::Pass))
        .at(d(1, 5, 10), Rebase(Ci::Pass))
        // Rebased by the bot when main moved, a few minutes ago.
        .at(m(7), Rebase(Ci::Running))
        .status(S::CiStillRunning)
}

fn courier_websocket() -> MrSpec {
    MrSpec::new("courier-ws", "feat(tracking): Live courier positions over WebSocket", &JULIEN)
        .branch("feat/4330-courier-websocket", "main")
        .milestone("v2.5.0")
        .labels(&["feature", "tracking"])
        .about("The tracking page polls every 30 s. With `/ws/shipments/{number}`, it gets the courier's position as the handheld reports it (every 10 s while out for delivery).")
        .file(added("src/tracking/ws.rs", r#"
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio::sync::broadcast::error::RecvError;

pub async fn upgrade(ws: WebSocketUpgrade, State(hub): State<PositionHub>, Path(number): Path<String>) -> Response {
    ws.on_upgrade(move |socket| follow(socket, hub, number))
}

/// Forwards the courier's positions until the client leaves or the parcel is delivered.
async fn follow(mut socket: WebSocket, hub: PositionHub, number: String) {
    let mut positions = hub.subscribe(&number);
    loop {
        match positions.recv().await {
            Ok(position) => {
                let text = serde_json::to_string(&position).expect("positions serialize");
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
            // A slow client skips to the latest position rather than being dropped.
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => break,
        }
    }
}
"#))
        .file(patch("src/events/bus.rs", &[(34, r#"
     ApiKeyRevoked(ApiKeyId),
+    CourierMoved(CourierId, GeoPoint),
 }
"#)]))
        .file(patch("src/api/mod.rs", &[(44, r#"
         .route("/public/track/:number", get(public::track))
+        .route("/ws/shipments/:number", get(tracking::ws::upgrade))
"#)]))
        .file(round_fixture())
        .at(d(5, 9, 0), Open)
        .at(d(5, 9, 1), Push(&["feat(tracking): WebSocket endpoint for courier positions", "test(tracking): Replay a recorded round"], Ci::Pass))
        .at(d(5, 9, 4), Ask(&MARINA))
        .at(d(5, 9, 4), Ask(&ALEX))
        .at(d(4, 11, 0), Note(&MARINA, "lag", Some(("src/tracking/ws.rs", 20)), "A slow client makes the broadcast channel lag: skip to the latest position rather than closing the connection?"))
        .at(d(3, 9, 40), Push(&["fix(tracking): Slow clients skip to the latest position"], Ci::Pass))
        .at(d(3, 9, 42), Reply(&JULIEN, "lag", "Lagged clients skip ahead now; the test replays a round with a client reading every 5 s."))
        .at(d(3, 10, 30), Resolve(&MARINA, "lag"))
        .at(d(3, 10, 31), Approve(&MARINA))
}

/// A courier's round recorded in Lyon, replayed by the WebSocket test.
fn round_fixture() -> FileChange {
    let mut text = String::from("[\n");
    let (mut lat, mut lon) = (45.76058_f64, 4.83590_f64);
    for i in 0..56u32 {
        let h = super::clock::hash(&["round", &i.to_string()]);
        lat += ((h % 21) as f64 - 9.0) * 0.00011;
        lon += ((h / 21 % 23) as f64 - 8.0) * 0.00013;
        let comma = if i == 55 { "" } else { "," };
        text.push_str(&format!(
            "  {{ \"courier\": \"c-1042\", \"at\": \"2025-03-18T09:{:02}:{:02}Z\", \"lat\": {lat:.5}, \"lon\": {lon:.5} }}{comma}\n",
            12 + i / 6,
            (i % 6) * 10
        ));
    }
    text.push(']');
    added("tests/fixtures/round_lyon_c1042.json", &text)
}

/// The partner's relay points, as the migration inserts them.
fn relay_points() -> FileChange {
    const CITIES: [(&str, &str, f64, f64); 40] = [
        ("Paris", "750", 48.8566, 2.3522),
        ("Marseille", "130", 43.2965, 5.3698),
        ("Lyon", "690", 45.7640, 4.8357),
        ("Toulouse", "310", 43.6047, 1.4442),
        ("Nice", "060", 43.7102, 7.2620),
        ("Nantes", "440", 47.2184, -1.5536),
        ("Montpellier", "340", 43.6108, 3.8767),
        ("Strasbourg", "670", 48.5734, 7.7521),
        ("Bordeaux", "330", 44.8378, -0.5792),
        ("Lille", "590", 50.6292, 3.0573),
        ("Rennes", "350", 48.1173, -1.6778),
        ("Reims", "510", 49.2583, 4.0317),
        ("Toulon", "830", 43.1242, 5.9280),
        ("Saint-Étienne", "420", 45.4397, 4.3872),
        ("Le Havre", "766", 49.4944, 0.1079),
        ("Grenoble", "380", 45.1885, 5.7245),
        ("Dijon", "210", 47.3220, 5.0415),
        ("Angers", "490", 47.4784, -0.5632),
        ("Nîmes", "300", 43.8367, 4.3601),
        ("Clermont-Ferrand", "630", 45.7772, 3.0870),
        ("Le Mans", "720", 48.0061, 0.1996),
        ("Aix-en-Provence", "131", 43.5297, 5.4474),
        ("Brest", "292", 48.3904, -4.4861),
        ("Tours", "370", 47.3941, 0.6848),
        ("Amiens", "800", 49.8941, 2.2958),
        ("Limoges", "870", 45.8336, 1.2611),
        ("Annecy", "740", 45.8992, 6.1294),
        ("Perpignan", "660", 42.6887, 2.8948),
        ("Metz", "570", 49.1193, 6.1757),
        ("Besançon", "250", 47.2378, 6.0241),
        ("Orléans", "450", 47.9030, 1.9093),
        ("Rouen", "760", 49.4432, 1.0999),
        ("Mulhouse", "681", 47.7508, 7.3359),
        ("Caen", "140", 49.1829, -0.3707),
        ("Nancy", "540", 48.6921, 6.1844),
        ("Avignon", "840", 43.9493, 4.8055),
        ("Poitiers", "860", 46.5802, 0.3404),
        ("La Rochelle", "170", 46.1603, -1.1511),
        ("Pau", "640", 43.2951, -0.3708),
        ("Bayonne", "641", 43.4929, -1.4748),
    ];
    const SHOPS: [&str; 8] = [
        "Tabac Presse",
        "Pressing",
        "Fleuriste",
        "Librairie",
        "Épicerie",
        "Cordonnerie",
        "Bureau de tabac",
        "Supérette",
    ];
    let mut text =
        String::from("INSERT INTO relay_points (code, name, postcode, city, lat, lon) VALUES\n");
    let mut n = 0;
    for (city, prefix, lat, lon) in CITIES {
        for k in 0..60u32 {
            n += 1;
            let h = super::clock::hash(&[city, &k.to_string()]);
            let dlat = ((h % 2001) as f64 - 1000.0) / 25_000.0;
            let dlon = ((h / 2001 % 2001) as f64 - 1000.0) / 18_000.0;
            let end = if n == 2400 { ";" } else { "," };
            text.push_str(&format!(
                "  ('RP{n:05}', '{} {}', '{prefix}{:02}', '{}', {:.5}, {:.5}){end}\n",
                SHOPS[(h % 8) as usize].replace('\'', "''"),
                k + 1,
                1 + h % 20,
                city.replace('\'', "''"),
                lat + dlat,
                lon + dlon
            ));
        }
    }
    added("migrations/0048_relay_points.sql", &text)
}

/// The OpenAPI description of the v2 routes.
fn openapi_spec() -> FileChange {
    const ROUTES: [(&str, &str, &str, &str); 19] = [
        (
            "/v2/shipments",
            "get",
            "listShipments",
            "The caller's shipments, newest first, a page at a time.",
        ),
        (
            "/v2/shipments",
            "post",
            "createShipment",
            "Creates a shipment and books its pickup.",
        ),
        (
            "/v2/shipments/{id}",
            "get",
            "getShipment",
            "One shipment with its carrier and last scan.",
        ),
        (
            "/v2/shipments/{id}",
            "patch",
            "updateShipment",
            "Changes the recipient or the options before pickup.",
        ),
        (
            "/v2/shipments/{id}",
            "delete",
            "cancelShipment",
            "Cancels a shipment that is not picked up yet.",
        ),
        (
            "/v2/shipments/{id}/events",
            "get",
            "listShipmentEvents",
            "The scans of a shipment, oldest first.",
        ),
        (
            "/v2/shipments/{id}/label",
            "get",
            "getLabel",
            "The shipping label, as PDF or ZPL.",
        ),
        (
            "/v2/shipments/{id}/return",
            "post",
            "createReturn",
            "A return shipment to the sender.",
        ),
        (
            "/v2/quotes",
            "post",
            "createQuote",
            "The price of a shipment before booking it.",
        ),
        (
            "/v2/pickups",
            "get",
            "listPickups",
            "The pickups booked for the coming days.",
        ),
        (
            "/v2/pickups/{id}",
            "delete",
            "cancelPickup",
            "Cancels a pickup.",
        ),
        (
            "/v2/relay-points",
            "get",
            "searchRelayPoints",
            "The relay points near a postcode.",
        ),
        (
            "/v2/webhooks",
            "get",
            "listWebhooks",
            "The caller's webhook endpoints.",
        ),
        (
            "/v2/webhooks",
            "post",
            "createWebhook",
            "Registers an endpoint for shipment events.",
        ),
        (
            "/v2/webhooks/{id}",
            "delete",
            "deleteWebhook",
            "Stops sending events to an endpoint.",
        ),
        (
            "/v2/webhooks/{id}/deliveries",
            "get",
            "listDeliveries",
            "The last deliveries to an endpoint and their answers.",
        ),
        (
            "/v2/api-keys",
            "get",
            "listApiKeys",
            "The account's API keys, without their secret.",
        ),
        (
            "/v2/api-keys",
            "post",
            "createApiKey",
            "A new API key; its secret is shown once.",
        ),
        (
            "/public/track/{number}",
            "get",
            "trackPublic",
            "What a recipient may see of a parcel, without an account.",
        ),
    ];
    const SCHEMAS: [(&str, &[(&str, &str)]); 10] = [
        (
            "Shipment",
            &[
                ("id", "string"),
                ("number", "string"),
                ("status", "ShipmentStatus"),
                ("createdAt", "date-time"),
                ("carrier", "Carrier"),
                ("weightKg", "number"),
                ("eta", "Eta"),
                ("recipient", "Address"),
                ("lastScan", "Scan"),
            ],
        ),
        (
            "NewShipment",
            &[
                ("recipient", "Address"),
                ("parcels", "Parcel[]"),
                ("carrier", "string"),
                ("options", "string[]"),
                ("reference", "string"),
            ],
        ),
        (
            "Parcel",
            &[
                ("weightKg", "number"),
                ("lengthCm", "integer"),
                ("widthCm", "integer"),
                ("heightCm", "integer"),
            ],
        ),
        (
            "Address",
            &[
                ("name", "string"),
                ("line1", "string"),
                ("line2", "string"),
                ("postcode", "string"),
                ("city", "string"),
                ("country", "string"),
                ("phone", "string"),
            ],
        ),
        (
            "Scan",
            &[
                ("id", "string"),
                ("scannedAt", "date-time"),
                ("kind", "ScanKind"),
                ("depot", "string"),
                ("lat", "number"),
                ("lon", "number"),
            ],
        ),
        (
            "Carrier",
            &[
                ("code", "string"),
                ("name", "string"),
                ("trackingUrl", "string"),
            ],
        ),
        (
            "Eta",
            &[("date", "date"), ("from", "string"), ("to", "string")],
        ),
        (
            "Quote",
            &[
                ("total", "number"),
                ("currency", "string"),
                ("weightKg", "number"),
                ("validUntil", "date-time"),
            ],
        ),
        (
            "Webhook",
            &[
                ("id", "string"),
                ("url", "string"),
                ("events", "string[]"),
                ("createdAt", "date-time"),
            ],
        ),
        ("Error", &[("error", "string"), ("requestId", "string")]),
    ];
    let mut y = String::from("openapi: 3.1.0\ninfo:\n  title: Orbit API\n  version: 2.3.0\n  description: Shipments, quotes, pickups and webhooks for the partners of Orbit Logistics.\nservers:\n  - url: https://api.orbit-logistics.dev\nsecurity:\n  - apiKey: []\npaths:\n");
    let mut last = "";
    for (path, method, id, summary) in ROUTES {
        if path != last {
            y.push_str(&format!("  {path}:\n"));
            last = path;
        }
        y.push_str(&format!(
            "    {method}:\n      operationId: {id}\n      summary: {summary}\n      tags: [{}]\n",
            path.split('/').nth(2).unwrap_or("public")
        ));
        if path.contains('{') {
            let name = path
                .rsplit('{')
                .next()
                .unwrap_or("id}")
                .trim_end_matches(|c| c == '}' || c == '/')
                .split('}')
                .next()
                .unwrap_or("id");
            y.push_str(&format!("      parameters:\n        - name: {name}\n          in: path\n          required: true\n          schema:\n            type: string\n"));
        }
        if method == "get" && !path.contains('{') {
            y.push_str("        - name: first\n          in: query\n          schema:\n            type: integer\n            maximum: 100\n        - name: after\n          in: query\n          schema:\n            type: string\n");
        }
        if matches!(method, "post" | "patch") {
            y.push_str("      requestBody:\n        required: true\n        content:\n          application/json:\n            schema:\n              $ref: '#/components/schemas/NewShipment'\n");
        }
        let ok = match method {
            "post" => "201",
            "delete" => "204",
            _ => "200",
        };
        y.push_str(&format!(
            "      responses:\n        '{ok}':\n          description: OK\n"
        ));
        if ok != "204" {
            y.push_str("          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Shipment'\n");
        }
        for (code, what) in [
            ("401", "The API key is missing or revoked."),
            ("404", "No such resource for this account."),
            ("429", "Too many requests: see Retry-After."),
        ] {
            y.push_str(&format!("        '{code}':\n          description: {what}\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Error'\n"));
        }
    }
    y.push_str("components:\n  securitySchemes:\n    apiKey:\n      type: apiKey\n      in: header\n      name: Authorization\n  schemas:\n");
    for (name, fields) in SCHEMAS {
        y.push_str(&format!(
            "    {name}:\n      type: object\n      properties:\n"
        ));
        for (field, ty) in fields.iter() {
            let (ty, format) = match *ty {
                "date-time" | "date" => ("string", Some(*ty)),
                "string" | "number" | "integer" => (*ty, None),
                t if t.ends_with("[]") => ("array", None),
                _ => ("object", None),
            };
            y.push_str(&format!("        {field}:\n          type: {ty}\n"));
            if let Some(format) = format {
                y.push_str(&format!("          format: {format}\n"));
            }
        }
    }
    added("openapi.yaml", &y)
}

/// The JSON snapshots of the v2 responses.
fn snapshots() -> FileChange {
    let mut text = String::new();
    for (k, (status, carrier, city)) in [
        ("in_transit", "colissimo", "Lyon"),
        ("out_for_delivery", "chronopost", "Nantes"),
        ("delivered", "dpd", "Lille"),
        ("returned", "colissimo", "Bordeaux"),
        ("created", "gls", "Rennes"),
        ("picked_up", "dpd", "Grenoble"),
        ("failed_attempt", "chronopost", "Toulouse"),
        ("at_relay_point", "colissimo", "Nice"),
        ("customs_hold", "dhl", "Strasbourg"),
        ("lost", "gls", "Metz"),
    ]
    .into_iter()
    .enumerate()
    {
        text.push_str(&format!("---\nsource: tests/api_v2.rs\nexpression: shipment_{status}\n---\n{{\n  \"id\": \"shp_01J8{:04}\",\n  \"number\": \"OR{:09}FR\",\n  \"status\": \"{status}\",\n  \"createdAt\": \"[date]\",\n  \"carrier\": {{\n    \"code\": \"{carrier}\",\n    \"trackingUrl\": \"https://track.orbit-logistics.dev/OR{:09}FR\"\n  }},\n  \"weightKg\": {:.1},\n  \"recipient\": {{\n    \"city\": \"{city}\",\n    \"country\": \"FR\"\n  }},\n  \"lastScan\": {{\n    \"kind\": \"depot_in\",\n    \"scannedAt\": \"[date]\",\n    \"depot\": \"{}\"\n  }}\n}}\n", 1200 + k, 482_115_300 + k * 7, 482_115_300 + k * 7, 0.8 + k as f64 * 1.7, &city[..3].to_uppercase()));
    }
    added("tests/snapshots/api_v2__shipments.snap", &text)
}

fn tickets() -> Vec<TicketSpec> {
    vec![
        TicketSpec {
            id: 4271,
            subject: "Expose shipments over GraphQL for the partners",
            tracker: "Feature",
            status: "In Progress",
            priority: "High",
            version: Some("v2.4.0"),
            author: &THOMAS,
            assignee: &MARINA,
            estimate_hours: Some(24.0),
            done: 80,
            start: d(10, 9, 0),
            entries: vec![
                (d(9, 18, 0), 6.0, 0, "Schema and resolvers", &MARINA),
                (d(8, 17, 0), 1.5, 1, "Review", &ALEX),
                (d(7, 18, 0), 4.0, 0, "DataLoader for the scans", &MARINA),
                (d(3, 18, 0), 3.5, 0, "Cursor pagination", &MARINA),
                (d(2, 12, 0), 1.0, 0, "Page size cap", &MARINA),
                (d(2, 10, 0), 0.75, 1, "Second review", &ALEX),
            ],
        },
        TicketSpec {
            id: 3987,
            subject: "Connection pool exhausted under load: requests wait 30 s",
            tracker: "Bug",
            status: "Assigned",
            priority: "Urgent",
            version: Some("v2.4.0"),
            author: &NADIA,
            assignee: &THOMAS,
            estimate_hours: Some(6.0),
            done: 60,
            start: d(14, 9, 0),
            entries: vec![
                (
                    d(13, 18, 0),
                    2.5,
                    0,
                    "Reproduced with 200 concurrent bookings",
                    &THOMAS,
                ),
                (
                    d(12, 18, 0),
                    3.0,
                    0,
                    "Release on drop, timeout from the settings",
                    &THOMAS,
                ),
                (d(12, 12, 0), 1.5, 1, "Review", &NADIA),
                (d(11, 12, 0), 1.0, 1, "Review", &SARAH),
            ],
        },
        TicketSpec {
            id: 4302,
            subject: "Push notifications on delivery status changes",
            tracker: "Feature",
            status: "In Progress",
            priority: "Normal",
            version: Some("v2.5.0"),
            author: &MARINA,
            assignee: &JULIEN,
            estimate_hours: Some(32.0),
            done: 50,
            start: d(11, 9, 0),
            entries: vec![
                (d(10, 18, 0), 7.0, 0, "FCM and APNs clients", &JULIEN),
                (d(7, 18, 0), 3.0, 0, "Status change only", &JULIEN),
                (d(6, 18, 0), 6.0, 0, "Preferences endpoint", &JULIEN),
                (d(3, 18, 0), 2.5, 0, "Mute per shipment", &JULIEN),
                (d(8, 16, 0), 1.0, 1, "Review", &THOMAS),
            ],
        },
        TicketSpec {
            id: 4315,
            subject: "Rate limiting per API key",
            tracker: "Feature",
            status: "Feedback",
            priority: "High",
            version: Some("v2.4.0"),
            author: &THOMAS,
            assignee: &ALEX,
            estimate_hours: Some(10.0),
            done: 90,
            start: d(8, 9, 0),
            entries: vec![
                (d(7, 18, 0), 5.0, 0, "Token bucket and headers", &ALEX),
                (d(5, 12, 0), 1.0, 0, "Burst per plan", &ALEX),
                (d(6, 16, 0), 0.5, 1, "Review", &THOMAS),
            ],
        },
        TicketSpec {
            id: 4188,
            subject: "OAuth2 PKCE sign-in for the mobile app",
            tracker: "Feature",
            status: "Resolved",
            priority: "High",
            version: Some("v2.4.0"),
            author: &MARINA,
            assignee: &ALEX,
            estimate_hours: Some(12.0),
            done: 100,
            start: d(9, 9, 0),
            entries: vec![
                (
                    d(8, 18, 0),
                    6.0,
                    0,
                    "Authorization code flow with PKCE",
                    &ALEX,
                ),
                (d(6, 12, 0), 1.0, 1, "Review", &THOMAS),
                (d(4, 18, 0), 2.0, 0, "Verifier kept across restarts", &SARAH),
                (d(3, 17, 0), 1.5, 2, "Tested on Android 12 to 15", &SARAH),
            ],
        },
        TicketSpec {
            id: 4350,
            subject: "Partners ask how failed webhooks are retried",
            tracker: "Support",
            status: "New",
            priority: "Low",
            version: None,
            author: &SARAH,
            assignee: &THOMAS,
            estimate_hours: Some(2.0),
            done: 0,
            start: d(2, 9, 0),
            entries: vec![],
        },
        TicketSpec {
            id: 4290,
            subject: "Audit #7: API keys can only be revoked by support",
            tracker: "Bug",
            status: "Feedback",
            priority: "High",
            version: None,
            author: &NADIA,
            assignee: &ALEX,
            estimate_hours: Some(8.0),
            done: 90,
            start: d(10, 9, 0),
            entries: vec![
                (d(9, 18, 0), 4.0, 0, "Revoke endpoint and button", &ALEX),
                (d(6, 18, 0), 2.5, 0, "Cache eviction, audit log", &ALEX),
                (d(7, 15, 0), 1.0, 1, "Review", &THOMAS),
            ],
        },
        TicketSpec {
            id: 4277,
            subject: "Invoices off by a cent against the accounting export",
            tracker: "Bug",
            status: "New",
            priority: "Normal",
            version: Some("v2.5.0"),
            author: &MARINA,
            assignee: &SARAH,
            estimate_hours: Some(3.0),
            done: 30,
            start: d(4, 9, 0),
            entries: vec![(d(3, 18, 0), 1.5, 0, "Per-line rounding", &SARAH)],
        },
        TicketSpec {
            id: 4320,
            subject: "Print shipping labels as ZPL on the depots' Zebra printers",
            tracker: "Feature",
            status: "In Progress",
            priority: "Normal",
            version: Some("v2.4.0"),
            author: &NADIA,
            assignee: &SARAH,
            estimate_hours: Some(8.0),
            done: 70,
            start: d(6, 9, 0),
            entries: vec![
                (d(5, 18, 0), 4.0, 0, "ZPL renderer", &SARAH),
                (d(3, 12, 0), 1.5, 0, "Resolution per depot", &SARAH),
                (d(4, 10, 0), 0.5, 1, "Review", &ALEX),
            ],
        },
    ]
}
