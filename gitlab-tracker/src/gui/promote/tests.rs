use super::*;

#[tokio::test]
async fn every_mr_lands_in_one_stage_by_its_branches() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let (from, to) = ("staging", "production");
    for r in rows(&app, from, to) {
        let arrived = |p: Option<BranchPresence>| p.is_some_and(|p| !p.is_missing());
        match r.stage {
            Stage::Done => assert!(arrived(r.on_to), "!{}", r.id),
            Stage::Ready => assert!(arrived(r.on_from) && !arrived(r.on_to), "!{}", r.id),
            Stage::Upstream => assert!(!arrived(r.on_from) && !arrived(r.on_to), "!{}", r.id),
        }
        // Only what is not on the first branch yet says what holds it.
        if r.stage != Stage::Upstream {
            assert!(r.blockers.is_empty());
        }
    }
}

#[test]
fn a_status_reads_in_sentence_case_with_its_acronyms() {
    assert_eq!(sentence_case("CHANGES REQ."), "Changes req.");
    assert_eq!(sentence_case("CI RUNNING"), "CI running");
    assert_eq!(sentence_case("CI MUST PASS"), "CI must pass");
    assert_eq!(sentence_case("SYNC FAILED"), "Sync failed");
    assert_eq!(sentence_case(""), "");
}
