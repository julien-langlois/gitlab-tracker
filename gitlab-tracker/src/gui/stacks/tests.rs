use super::*;

#[tokio::test]
async fn the_demo_stacks() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let all = stacks(&app);
    // Push notifications ← preferences ← mute, and the PKCE verifier on
    // the merged PKCE MR.
    let iid = crate::demo::iid;
    let chain = all
        .iter()
        .find(|s| s.root().id == iid("push-notifications"))
        .expect("the notifications stack");
    let ids: Vec<String> = chain.nodes.iter().map(|n| n.id.clone()).collect();
    assert_eq!(
        ids,
        [
            iid("push-notifications"),
            iid("notif-preferences"),
            iid("notif-mute")
        ]
    );
    assert_eq!(chain.base, "main");
    let mute = &chain.nodes[2];
    assert_eq!(mute.depth, 2);
    assert_eq!(mute.parent, Some(iid("notif-preferences")));
    let on_merged = all
        .iter()
        .find(|s| s.root().id == iid("oauth-pkce"))
        .expect("the stack on a merged MR");
    assert!(on_merged.nodes[1]
        .problems
        .iter()
        .any(|p| matches!(p, Problem::Retarget { .. })));
    // The stack to retarget comes first.
    assert_eq!(all[0].root().id, iid("oauth-pkce"));
}
