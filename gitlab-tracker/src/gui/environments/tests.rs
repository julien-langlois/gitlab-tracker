use super::*;

#[tokio::test]
async fn the_demo_overview_follows_the_flow() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    let model = model(&app, Some(&overview));
    assert_eq!(model.branches.len(), app.branches.len());
    // Each tracked branch has an environment and a last deployment.
    assert!(model
        .branches
        .iter()
        .all(|b| !b.environments.is_empty() && b.last.is_some()));
    // Production last.
    assert_eq!(model.branches.last().unwrap().role, BranchRole::Production);
    assert!(model.pair(None).is_some());
}
