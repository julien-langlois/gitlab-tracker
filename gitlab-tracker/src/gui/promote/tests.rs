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
