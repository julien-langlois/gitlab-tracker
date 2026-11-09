//! Orbit Mobile: the Android app of the same platform, in another tab. Git
//! flow with a semi-linear history (an MR behind its target needs a rebase):
//! `develop` cut to `preprod` for each release, released to the store from
//! `master`, a redesign on a long-lived feature branch, and a hotfix merged
//! into `master` that has yet to come back down to `develop`.

use super::clock::{ahead, d, m, When};
use super::model::Ev::*;
use super::model::*;
use super::orbit_api::{JULIEN, MARINA, THOMAS};
use crate::models::MergeabilityStatus as S;

pub const PROJECT_ID: &str = "4388";

pub static LEA: Person = Person {
    name: "Léa Fontaine",
    login: "lea_f",
};
pub static OMAR: Person = Person {
    name: "Omar Benali",
    login: "omar_b",
};

fn kotlin(path: &'static str, start: u32, body: &'static str) -> FileChange {
    patch(path, &[(start, body)])
}

pub fn spec() -> ProjectSpec {
    let sync = |at: When, from: &'static str, to: &'static str, by: &'static Person| Sync {
        at,
        from,
        to,
        by,
        deploy: Deploy::Ok,
    };
    let mut flow = std::collections::BTreeMap::new();
    flow.insert(
        "redesign_develop".to_string(),
        crate::branch_flow::BranchSetting {
            role: Some(crate::branch_flow::BranchRole::Feature),
            parent: Some("develop".into()),
            alias: Some("redesign".into()),
        },
    );
    let mut mrs = history();
    mrs.extend(in_review());
    ProjectSpec {
        id: PROJECT_ID,
        name: "Orbit Mobile",
        path: "orbit/orbit-android",
        branches: vec!["develop", "preprod", "master", "redesign_develop"],
        flow: Some(flow),
        environments: vec![
            ("develop", "internal"),
            ("preprod", "beta"),
            ("master", "production"),
        ],
        jobs: vec![
            ("ktlint", "check", 64.0),
            ("android-unit", "test", 188.0),
            ("ui-tests", "test", 452.0),
            ("assemble", "build", 236.0),
        ],
        first_iid: 512,
        mrs,
        syncs: vec![
            sync(d(14, 11, 0), "develop", "preprod", &LEA),
            sync(d(10, 14, 0), "preprod", "master", &LEA),
            sync(d(9, 9, 0), "master", "develop", &LEA),
            sync(d(12, 9, 30), "develop", "redesign_develop", &OMAR),
            sync(d(6, 9, 30), "develop", "redesign_develop", &OMAR),
            sync(d(4, 11, 0), "develop", "preprod", &LEA),
        ],
        copies: Vec::new(),
        directs: vec![Direct {
            at: d(4, 10, 40),
            branch: "develop",
            title: "chore(release): Version 3.8.0 (380)",
            by: &LEA,
        }],
        branch_ci: Vec::new(),
        milestones: vec![
            MilestoneSpec {
                title: "3.7.0",
                due: Some(d(10, 18, 0)),
                description: "Apple Pay and Google Pay at checkout.",
            },
            MilestoneSpec {
                title: "3.8.0",
                due: Some(ahead(3, 18, 0)),
                description: "Sign-in with PKCE, account deletion (store requirement).",
            },
            MilestoneSpec {
                title: "3.9.0",
                due: Some(ahead(13, 18, 0)),
                description: "Live courier on the map, GraphQL for the lists.",
            },
        ],
        tickets: vec![
            TicketSpec {
                id: 4401,
                subject: "Show the courier live on the tracking map",
                tracker: "Feature",
                status: "In Progress",
                priority: "High",
                version: Some("3.9.0"),
                author: &MARINA,
                assignee: &LEA,
                estimate_hours: Some(20.0),
                done: 60,
                start: d(6, 9, 0),
                entries: vec![
                    (d(5, 18, 0), 6.0, 0, "Map layer and WebSocket client", &LEA),
                    (d(3, 18, 0), 4.5, 0, "Reconnects with backoff", &LEA),
                    (d(2, 16, 0), 1.0, 1, "Review", &OMAR),
                ],
            },
            TicketSpec {
                id: 4420,
                subject: "Crash on checkout when the promo code field is empty",
                tracker: "Bug",
                status: "Assigned",
                priority: "Urgent",
                version: Some("3.8.0"),
                author: &JULIEN,
                assignee: &OMAR,
                estimate_hours: Some(2.0),
                done: 80,
                start: d(1, 9, 0),
                entries: vec![(d(1, 17, 0), 1.5, 0, "Null check and test", &OMAR)],
            },
            TicketSpec {
                id: 4415,
                subject: "Dark theme for the tracking screens",
                tracker: "Feature",
                status: "Feedback",
                priority: "Normal",
                version: Some("3.9.0"),
                author: &LEA,
                assignee: &OMAR,
                estimate_hours: Some(6.0),
                done: 90,
                start: d(5, 9, 0),
                entries: vec![(d(4, 18, 0), 5.0, 0, "Colours and map style", &OMAR)],
            },
        ],
        rules: vec![
            RuleSpec {
                name: "Mobile reviewers",
                required: 1,
                eligible: vec![&LEA, &OMAR, &JULIEN],
                paths: Vec::new(),
            },
            RuleSpec {
                name: "API owners",
                required: 1,
                eligible: vec![&THOMAS],
                paths: vec!["app/src/main/graphql/"],
            },
        ],
        last_session: d(2, 18, 30),
        profile: crate::models::DifficultyProfile {
            name: "Android".into(),
            easy_threshold: 30,
            hard_threshold: 200,
        },
    }
}

fn history() -> Vec<MrSpec> {
    vec![
        MrSpec::new("wallet-pay", "feat(checkout): Apple Pay and Google Pay", &LEA)
            .branch("feature/wallet-payments", "develop")
            .milestone("3.7.0")
            .labels(&["feature"])
            .about("Google Pay at checkout through the payment provider's SDK.")
            .file(kotlin("app/src/main/java/com/orbit/app/checkout/CheckoutScreen.kt", 58, r#"
     Column {
         CardForm(state.card, onChange = viewModel::onCard)
+        if (state.googlePayReady) {
+            GooglePayButton(onClick = { viewModel.payWithGoogle(activity) })
+        }
     }
"#))
            .shipped(&[&OMAR], d(22, 10, 0), d(20, 14, 0), d(18, 11, 0), d(17, 10, 0),
                ("Hide the button when `isReadyToPay` says no: some devices have no wallet.", "Hidden unless Google Pay is ready."),
                Some("fix(checkout): Only show Google Pay when ready")),
        MrSpec::new("map-offline", "fix(tracking): Map froze when the courier went offline", &OMAR)
            .branch("bugfix/map-offline-freeze", "develop")
            .milestone("3.7.0")
            .labels(&["bug"])
            .about("The map waited forever on the last position when the handheld lost its network.")
            .file(kotlin("app/src/main/java/com/orbit/app/tracking/TrackingViewModel.kt", 41, r#"
-        positions.collect { position -> _state.update { it.copy(courier = position) } }
+        positions
+            .timeout(2.minutes)
+            .catch { _state.update { it.copy(courierOffline = true) } }
+            .collect { position -> _state.update { it.copy(courier = position, courierOffline = false) } }
"#))
            .shipped(&[&LEA], d(18, 9, 0), d(17, 10, 0), d(16, 15, 0), d(16, 15, 30),
                ("Show something when it times out, not just a frozen marker.", "\"Position not updated for 2 min\" under the map."),
                None),
        MrSpec::new("compose-bom", "chore(deps): Update the Compose BOM to 2024.10.01", &OMAR)
            .branch("chore/compose-bom-2024-10", "develop")
            .milestone("3.7.0")
            .labels(&["dependencies"])
            .about("Fixes the ripple drawn outside rounded buttons.")
            .file(kotlin("gradle/libs.versions.toml", 4, r#"
-compose-bom = "2024.09.03"
+compose-bom = "2024.10.01"
"#))
            .shipped(&[&LEA], d(16, 9, 0), d(15, 10, 0), d(15, 10, 30), d(15, 11, 0),
                ("Screenshot tests still pass?", "Yes, 0 diff."),
                None),
        MrSpec::new("delete-account", "feat(account): Delete my account", &LEA)
            .branch("feature/delete-account", "develop")
            .milestone("3.8.0")
            .labels(&["feature", "store"])
            .about("The store requires deleting the account from the app.")
            .file(kotlin("app/src/main/java/com/orbit/app/account/AccountScreen.kt", 92, r#"
+        DangerButton(
+            text = stringResource(R.string.account_delete),
+            onClick = { confirmDelete = true },
+        )
"#))
            .shipped(&[&JULIEN], d(10, 9, 0), d(9, 14, 0), d(8, 10, 0), d(8, 10, 30),
                ("Ask for confirmation, and say the parcels in transit are still delivered.", "Dialog added with that sentence."),
                Some("feat(account): Confirm before deleting")),
        MrSpec::new("dutch-receipt", "fix(l10n): Missing Dutch strings on the receipt", &OMAR)
            .branch("bugfix/nl-receipt", "develop")
            .milestone("3.8.0")
            .labels(&["bug", "l10n"])
            .about("Dutch users saw the receipt in English.")
            .file(kotlin("app/src/main/res/values-nl/strings.xml", 120, r#"
+    <string name="receipt_title">Ontvangstbewijs</string>
+    <string name="receipt_total">Totaal</string>
+    <string name="receipt_vat">btw</string>
"#))
            .shipped(&[&LEA], d(7, 9, 30), d(6, 10, 0), d(6, 10, 30), d(6, 11, 0),
                ("\"btw\" is lowercase in Dutch receipts, right?", "Yes, checked with the Utrecht depot."),
                None),
        MrSpec::new("pkce-signin", "feat(auth): Sign in with PKCE", &LEA)
            .branch("feature/pkce-signin", "develop")
            .milestone("3.8.0")
            .labels(&["feature", "security"])
            .about("Uses the API's new OAuth2 PKCE flow; the stored password goes away.")
            .file(kotlin("app/src/main/java/com/orbit/app/auth/SignIn.kt", 20, r#"
-    suspend fun signIn(email: String, password: String) = api.login(email, password)
+    fun authorizationRequest(): AuthorizationRequest {
+        val verifier = CodeVerifierUtil.generateRandomCodeVerifier()
+        verifierStore.save(verifier)
+        return AuthorizationRequest.Builder(config, CLIENT_ID, ResponseTypeValues.CODE, REDIRECT)
+            .setCodeVerifier(verifier, CodeVerifierUtil.deriveCodeVerifierChallenge(verifier), "S256")
+            .build()
+    }
"#))
            .shipped(&[&OMAR], d(8, 9, 0), d(7, 11, 0), d(5, 10, 0), d(5, 10, 30),
                ("The verifier is lost when Android kills the app during the redirect.", "Kept server-side now, see the API's MR."),
                None),
        MrSpec::new("paging3", "perf(list): Paging 3 for the shipments list", &OMAR)
            .branch("perf/paging3", "develop")
            .milestone("3.8.0")
            .labels(&["performance"])
            .about("The list loaded every shipment at start: 4 s for the busiest senders.")
            .file(kotlin("app/src/main/java/com/orbit/app/shipments/ShipmentsViewModel.kt", 18, r#"
-    val shipments = flow { emit(api.allShipments()) }
+    val shipments = Pager(PagingConfig(pageSize = 30)) { ShipmentsPagingSource(api) }.flow.cachedIn(viewModelScope)
"#))
            .shipped(&[&LEA], d(5, 14, 0), d(4, 15, 0), d(3, 10, 0), d(3, 10, 40),
                ("Keep the scroll position when coming back from a shipment.", "`cachedIn` keeps it."),
                None),
        MrSpec::new("home-redesign", "feat(redesign): New home screen", &OMAR)
            .branch("feature/redesign-home", "redesign_develop")
            .labels(&["feature", "redesign"])
            .about("The new home screen of the redesign: the next delivery first, then the rest.")
            .file(kotlin("app/src/main/java/com/orbit/app/home/HomeScreen.kt", 1, r#"
+@Composable
+fun HomeScreen(state: HomeState) {
+    LazyColumn {
+        state.next?.let { item { NextDeliveryCard(it) } }
+        items(state.others) { ShipmentRow(it) }
+    }
+}
"#))
            .file(opaque("app/src/main/res/drawable-nodpi/home_empty.webp", true))
            .shipped(&[&LEA], d(9, 9, 0), d(8, 15, 0), d(7, 10, 0), d(7, 10, 30),
                ("Empty state when there is no delivery?", "Added an illustration and the \"send a parcel\" button."),
                Some("feat(redesign): Empty home state")),
        // Released straight from master; develop and preprod do not have it yet.
        MrSpec::new("push-wrong-parcel", "fix(push): Opening a notification showed the wrong parcel", &JULIEN)
            .branch("hotfix/push-wrong-parcel", "master")
            .labels(&["bug", "hotfix"])
            .about("The notification's intent reused the first parcel's extras: `PendingIntent` needs a request code per parcel.")
            .file(kotlin("app/src/main/java/com/orbit/app/push/Notifications.kt", 33, r#"
-        val intent = PendingIntent.getActivity(context, 0, open(shipment), FLAG_IMMUTABLE)
+        val intent = PendingIntent.getActivity(context, shipment.id.hashCode(), open(shipment), FLAG_IMMUTABLE or FLAG_UPDATE_CURRENT)
"#))
            .shipped(&[&LEA], d(2, 9, 30), d(2, 10, 0), d(2, 14, 30), d(2, 15, 0),
                ("FLAG_UPDATE_CURRENT too, or an old parcel's extras stay.", "Added."),
                None),
    ]
}

fn in_review() -> Vec<MrSpec> {
    vec![
        MrSpec::new("courier-map", "feat(tracking): Live courier on the map", &LEA)
            .branch("feature/4401-live-courier", "develop")
            .milestone("3.9.0")
            .ticket(4401)
            .labels(&["feature"])
            .about("Follows the courier through the API's new WebSocket while the parcel is out for delivery.\n\nRefs #4401")
            .file(kotlin("app/src/main/java/com/orbit/app/tracking/CourierSocket.kt", 1, r#"
+class CourierSocket(private val client: OkHttpClient, private val baseUrl: HttpUrl) {
+    fun positions(number: String): Flow<CourierPosition> = callbackFlow {
+        val socket = client.newWebSocket(request(number), object : WebSocketListener() {
+            override fun onMessage(webSocket: WebSocket, text: String) {
+                trySend(json.decodeFromString<CourierPosition>(text))
+            }
+            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) = close(t)
+        })
+        awaitClose { socket.close(1000, null) }
+    }.retryWhen { _, attempt -> delay(backoff(attempt)); true }
+}
"#))
            .at(d(5, 10, 0), Open)
            .at(d(5, 10, 1), Push(&["feat(tracking): Courier socket"], Ci::Pass))
            .at(d(5, 10, 5), Ask(&OMAR))
            .at(d(3, 15, 0), Push(&["feat(tracking): Reconnect with backoff"], Ci::Flaky("ui-tests")))
            .at(d(2, 15, 30), Note(&OMAR, "battery", Some(("app/src/main/java/com/orbit/app/tracking/CourierSocket.kt", 3)), "Close the socket when the screen goes to the background: it drains the battery otherwise."))
            .at(m(50), Push(&["fix(tracking): Close the socket in the background"], Ci::Pass))
            .at(m(48), Reply(&LEA, "battery", "Bound to the screen's lifecycle now."))
            // GitLab is still computing the merge status: the app asks again.
            .status(S::Retrying),
        MrSpec::new("graphql-list", "feat(api): Shipments list from the GraphQL endpoint", &MARINA)
            .branch("feature/graphql-shipments", "develop")
            .milestone("3.9.0")
            .labels(&["feature", "api"])
            .about("Reads the shipments list in one GraphQL query (with the last scan) instead of one call per shipment. Needs the API's GraphQL MR.")
            .file(added("app/src/main/graphql/Shipments.graphql", r#"
query Shipments($after: String) {
  shipments(first: 30, after: $after) {
    edges { node { number status eta { date } scans { scannedAt kind } } }
    pageInfo { hasNextPage endCursor }
  }
}
"#))
            .file(kotlin("app/src/main/java/com/orbit/app/shipments/ShipmentsPagingSource.kt", 14, r#"
-        val page = api.shipments(cursor = params.key)
+        val page = apollo.query(ShipmentsQuery(after = Optional.presentIfNotNull(params.key))).execute().dataOrThrow().shipments
"#))
            .at(d(3, 9, 0), Open)
            .at(d(3, 9, 1), Push(&["feat(api): Apollo client and the shipments query"], Ci::Pass))
            .at(d(3, 9, 5), Ask(&THOMAS))
            .at(d(3, 9, 5), Ask(&LEA))
            .at(d(2, 11, 0), Approve(&LEA)),
        MrSpec::new("dark-tracking", "feat(ui): Dark theme for the tracking screens", &OMAR)
            .branch("feature/4415-dark-tracking", "develop")
            .milestone("3.9.0")
            .ticket(4415)
            .labels(&["feature", "ui"])
            .about("The tracking screens and the map follow the system's dark theme.\n\nRefs #4415")
            .file(kotlin("app/src/main/java/com/orbit/app/tracking/TrackingMap.kt", 27, r#"
     GoogleMap(
         cameraPositionState = camera,
+        properties = MapProperties(mapStyleOptions = if (isSystemInDarkTheme()) darkMapStyle() else null),
     ) {
"#))
            .at(d(4, 14, 0), Open)
            .at(d(4, 14, 1), Push(&["feat(ui): Dark map style"], Ci::Pass))
            .at(d(4, 14, 5), Ask(&LEA))
            .at(d(3, 11, 0), Note(&LEA, "contrast", None, "The ETA text is grey on dark grey: below 4.5:1."))
            .at(d(2, 10, 0), Rebase(Ci::Pass))
            .at(d(2, 10, 5), Reply(&OMAR, "contrast", "Uses `onSurfaceVariant` now: 7.1:1."))
            .at(d(2, 11, 30), Resolve(&LEA, "contrast"))
            .at(d(2, 11, 31), Approve(&LEA))
            .status(S::Mergeable),
        MrSpec::new("promo-crash", "fix(checkout): Crash when the promo code field is empty", &OMAR)
            .branch("bugfix/4420-empty-promo-code", "develop")
            .milestone("3.8.0")
            .ticket(4420)
            .labels(&["bug", "crash"])
            .about("Top crash of 3.7.0 (0.8 % of sessions): applying an empty promo code.\n\nRefs #4420")
            .file(kotlin("app/src/main/java/com/orbit/app/checkout/CheckoutViewModel.kt", 77, r#"
     fun applyPromo(code: String) {
+        if (code.isBlank()) return
         viewModelScope.launch {
"#))
            .at(d(1, 16, 0), Open)
            .at(d(1, 16, 1), Push(&["fix(checkout): Ignore an empty promo code"], Ci::Pass))
            .at(d(1, 16, 5), Ask(&LEA))
            .at(d(1, 17, 20), Note(&LEA, "test", None, "A unit test for the blank code, so it does not come back?"))
            // Just pushed: the pipeline waits for a runner.
            .at(m(3), Push(&["test(checkout): Blank promo code"], Ci::Pending))
            .status(S::CiStillRunning),
        MrSpec::new("receipt-pdf", "feat(receipt): Share the receipt as PDF", &LEA)
            .branch("feature/receipt-pdf", "develop")
            .milestone("3.9.0")
            .labels(&["feature"])
            .about("Small businesses forward the receipt to their accountant.")
            .file(kotlin("app/src/main/java/com/orbit/app/receipt/ReceiptPdf.kt", 1, r#"
+fun Receipt.toPdf(context: Context): File {
+    val document = PdfDocument()
+    val page = document.startPage(PdfDocument.PageInfo.Builder(595, 842, 1).create())
+    ReceiptRenderer(this).draw(page.canvas)
+    document.finishPage(page)
+    return File(context.cacheDir, "receipt-$number.pdf").also { document.writeTo(it.outputStream()) }
+}
"#))
            .at(d(6, 10, 0), Open)
            .at(d(6, 10, 1), Push(&["feat(receipt): PDF export and share sheet"], Ci::Pass))
            .at(d(6, 10, 5), Ask(&OMAR))
            .at(d(5, 15, 0), Approve(&OMAR))
            .status(S::NeedsRebase),
        // Added a moment ago: its first read is on its way.
        MrSpec::new("home-widget", "feat(widget): Next delivery on a home screen widget", &OMAR)
            .branch("feature/home-widget", "develop")
            .milestone("3.9.0")
            .at(d(1, 11, 0), Open)
            .at(d(1, 11, 1), Push(&["feat(widget): Glance widget"], Ci::Pass))
            .fetch(Fetch::Loading),
    ]
}
