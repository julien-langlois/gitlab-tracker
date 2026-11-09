//! A job's log as the runner prints it: the runner's header, the commands of
//! the job, and their output for how it went (passed, failed on the MR's own
//! failure, still running).

use super::engine::{JobRec, Owner, PipeRec, World};

/// The commands a job runs and what they print when they pass.
fn script(job: &str) -> (&'static str, &'static str) {
    match job {
        "lint" => (
            "$ cargo fmt --all -- --check\n$ cargo clippy --workspace --all-targets -- -D warnings",
            "    Checking orbit-core v2.4.0-dev (/builds/orbit/api/crates/core)\n    Checking orbit-api v2.4.0-dev (/builds/orbit/api)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 41.87s",
        ),
        "unit-tests" => (
            "$ cargo nextest run --workspace --lib --profile ci",
            "    Starting 412 tests across 6 binaries\n        PASS [   0.012s] orbit-core shipment::tests::status_follows_the_scans\n        PASS [   0.031s] orbit-api auth::tests::expired_tokens_are_refused\n        PASS [   0.207s] orbit-api pricing::tests::quotes_round_per_line\n------------\n     Summary [  38.114s] 412 tests run: 412 passed, 3 skipped",
        ),
        "integration-tests" => (
            "$ sqlx migrate run\n$ cargo nextest run --workspace --test '*' --profile ci",
            "Applied 47/migrate shipments status index (12.3ms)\n    Starting 96 tests across 4 binaries\n        PASS [   1.402s] orbit-api::shipments create_then_track_a_parcel\n        PASS [   2.118s] orbit-api::webhooks retries_until_the_endpoint_answers\n------------\n     Summary [ 211.904s] 96 tests run: 96 passed, 0 skipped",
        ),
        "api-contract" => (
            "$ ./scripts/run-api.sh --background\n$ schemathesis run openapi.yaml --base-url http://localhost:8080 --checks all",
            "Schemathesis v3.36.2\nSchema location: openapi.yaml\nSpecification version: Open API 3.1.0\nCollected API operations: 38\n\n......................................\n\n========================= 38 passed in 104.61s =========================",
        ),
        "build-image" => (
            "$ docker buildx build --push --tag $CI_REGISTRY_IMAGE:$CI_COMMIT_SHORT_SHA .",
            "#14 [builder 6/6] RUN cargo build --release --locked\n#14 92.40     Finished `release` profile [optimized] target(s) in 1m 31s\n#18 exporting to image\n#18 pushing layers 4.2s done\n#18 DONE 9.8s",
        ),
        // The mobile app's jobs.
        "ktlint" => (
            "$ ./gradlew ktlintCheck detekt",
            "> Task :app:ktlintMainSourceSetCheck\n> Task :app:detekt\nBUILD SUCCESSFUL in 1m 2s\n14 actionable tasks: 14 executed",
        ),
        "android-unit" => (
            "$ ./gradlew testReleaseUnitTest",
            "> Task :app:testReleaseUnitTest\n\nTrackingViewModelTest > shows the courier on the map PASSED\nCheckoutViewModelTest > applies the promo code once PASSED\n\nBUILD SUCCESSFUL in 3m 12s\n318 tests completed, 0 failed",
        ),
        "ui-tests" => (
            "$ ./scripts/start-emulator.sh pixel_7_api_34\n$ ./gradlew connectedReleaseAndroidTest",
            "Starting 64 tests on pixel_7_api_34(AVD) - 14\n\npixel_7_api_34(AVD) - 14 Tests 64/64 completed. (0 skipped) (0 failed)\nFinished 64 tests on pixel_7_api_34(AVD) - 14\n\nBUILD SUCCESSFUL in 7m 48s",
        ),
        "assemble" => (
            "$ ./gradlew bundleRelease",
            "> Task :app:bundleRelease\n\nBUILD SUCCESSFUL in 4m 1s\nArtifacts: app/build/outputs/bundle/release/app-release.aab (21.4 MB)",
        ),
        _ => (
            "$ ./deploy.sh",
            "Deploying...\nDone.",
        ),
    }
}

/// How a job fails when nothing in the change is to blame: a flaky check.
fn usual_failure(job: &str) -> &'static str {
    match job {
        "api-contract" => "Schemathesis v3.36.2\nSchema location: openapi.yaml\nCollected API operations: 38\n\n.................E\n\n================================ ERRORS ================================\n__________________________ GET /v2/shipments/{id}/events __________________________\nrequests.exceptions.ConnectionError: HTTPConnectionPool(host='localhost', port=8080): Read timed out. (read timeout=10)\n\nThe API was still warming its connection pool: run the job again.\n=================== 17 passed, 1 errored in 98.20s ===================\nERROR: Job failed: exit code 1",
        "integration-tests" => "    Starting 96 tests across 4 binaries\n        FAIL [  30.004s] orbit-api::webhooks retries_until_the_endpoint_answers\n\n--- STDERR: orbit-api::webhooks retries_until_the_endpoint_answers ---\nthread 'retries_until_the_endpoint_answers' panicked at tests/webhooks.rs:88:10:\ntimed out waiting for the mock endpoint (30 s)\n------------\n     Summary [ 214.316s] 96 tests run: 95 passed, 1 failed, 0 skipped\nerror: test run failed\nERROR: Job failed: exit code 100",
        "ui-tests" => "Starting 64 tests on pixel_7_api_34(AVD) - 14\n\ncom.orbit.app.tracking.TrackingScreenTest > mapFollowsTheCourier[pixel_7_api_34(AVD) - 14] FAILED\n    androidx.test.espresso.NoMatchingViewException: No views in hierarchy found matching: with id: com.orbit.app:id/courier_marker\n    (the map tiles were still loading after 15 s)\n\nTests on pixel_7_api_34(AVD) - 14 failed: There was 1 failure(s).\nBUILD FAILED in 8m 2s\nERROR: Job failed: exit code 1",
        "lint" => "error: this `if` has identical blocks\n  --> src/api/shipments/mod.rs:57:9\n   |\n   = note: `-D clippy::if-same-then-else` implied by `-D warnings`\nerror: could not compile `orbit-api` (lib) due to 1 previous error\nERROR: Job failed: exit code 101",
        _ => "error: test failed, to rerun pass `--lib`\nERROR: Job failed: exit code 101",
    }
}

/// A deployment job: the release rolled out, or why it was not.
fn deploy(world: &World, job: &str, failed: bool, sha: &str) -> (String, String) {
    let env = job.trim_start_matches("deploy-");
    let short = &sha[..8.min(sha.len())];
    if world.spec.id == super::orbit_mobile::PROJECT_ID {
        let track = if env == "production" {
            "production"
        } else {
            "internal"
        };
        let out = if failed {
            format!("Uploading app-release.aab to the {track} track…\nERROR: Google Play Developer API: 403 The caller does not have permission (the service account key expired on rotation).\nERROR: Job failed: exit code 1")
        } else {
            format!("Uploading app-release.aab to the {track} track…\nRelease 3.8.0 ({short}) rolled out to 100% of the {track} track.\nJob succeeded")
        };
        return (
            format!("$ fastlane supply --track {track} --aab app/build/outputs/bundle/release/app-release.aab"),
            out,
        );
    }
    let command = format!(
        "$ helm upgrade --install orbit-api ./chart --namespace {env} --set image.tag={short} --atomic --timeout 5m"
    );
    let out = if failed && env == "staging" {
        format!("Error: UPGRADE FAILED: failed to pull image \"registry.orbit-logistics.dev/orbit/api:{short}\": rpc error: code = DeadlineExceeded desc = context deadline exceeded\nrelease orbit-api rolled back to its previous revision\nERROR: Job failed: exit code 1")
    } else if failed {
        "Running pre-upgrade hook: orbit-api-migrate\nError: pre-upgrade hooks failed: job orbit-api-migrate failed: BackoffLimitExceeded\n  migrate: error: while executing migration 46: canceling statement due to lock timeout\nrelease orbit-api rolled back to its previous revision\nERROR: Job failed: exit code 1".to_string()
    } else {
        format!("Release \"orbit-api\" has been upgraded. Happy Helming!\nNAME: orbit-api\nNAMESPACE: {env}\nSTATUS: deployed\nREVISION: {}\nJob succeeded", 40 + super::clock::hash(&[sha]) % 60)
    };
    (command, out)
}

pub fn log(world: &World, pipeline: &PipeRec, job: &JobRec) -> String {
    let short = &pipeline.sha[..8.min(pipeline.sha.len())];
    let reference = match &pipeline.owner {
        Owner::Mr(i) => format!("refs/merge-requests/{}/head", world.mrs[*i].iid),
        Owner::Branch(branch) => (*branch).to_string(),
    };
    let runner = 1 + super::clock::hash(&[&pipeline.sha, job.name]) % 4;
    let mut out = format!(
        "Running with gitlab-runner 17.5.2 (c6eae8f3)\n  on orbit-k8s-runner-{runner} t3_K9xfZ, system ID: r_8pQ2mV0tLw4n\nPreparing the \"kubernetes\" executor\nUsing Kubernetes namespace: gitlab-runners\nWaiting for pod gitlab-runners/runner-t3k9xfz-project-{}-concurrent-0 to be running, status is Pending\nGetting source from Git repository\nFetching changes with git depth set to 20...\nChecking out {short} as detached HEAD (ref is {reference})...\nExecuting \"step_script\" stage of the job script\n",
        world.spec.id
    );
    let failed = job.status == "failed";
    if job.name.starts_with("deploy-") {
        let (command, body) = deploy(world, job.name, failed, &pipeline.sha);
        out.push_str(&command);
        out.push('\n');
        out.push_str(&body);
        return out;
    }
    let (command, passed) = script(job.name);
    out.push_str(command);
    out.push('\n');
    match job.status {
        "running" => out.push_str(passed.lines().next().unwrap_or("Running…")),
        "failed" => {
            let own = match &pipeline.owner {
                Owner::Mr(i) => world.mrs[*i]
                    .spec
                    .failure
                    .filter(|(name, _)| *name == job.name)
                    .map(|(_, text)| text),
                Owner::Branch(_) => None,
            };
            out.push_str(own.unwrap_or_else(|| usual_failure(job.name)));
            if !out.ends_with("exit code 1") && !out.contains("ERROR: Job failed") {
                out.push_str("\nERROR: Job failed: exit code 1");
            }
        }
        _ => {
            out.push_str(passed);
            out.push_str("\nJob succeeded");
        }
    }
    out
}
