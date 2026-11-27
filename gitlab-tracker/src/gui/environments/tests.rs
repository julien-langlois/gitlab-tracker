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

#[tokio::test]
async fn each_environment_says_the_release_it_runs() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    let model = model(&app, Some(&overview));
    let version = |name: &str| {
        model
            .branches
            .iter()
            .find(|b| b.name == name)
            .and_then(|b| b.version.clone())
    };
    // Production runs the tag itself; staging's last good deployment too
    // (the two since failed).
    assert_eq!(version("production"), Some(("v2.3.3".into(), 0)));
    assert_eq!(version("staging"), Some(("v2.3.3".into(), 0)));
    // Main: the tag, and the MRs merged before its last deployment that
    // production lacks; the one merged since is not deployed.
    let (tag, since) = version("main").expect("main's version");
    assert_eq!(tag, "v2.3.3");
    assert_eq!(since, 4);
}
