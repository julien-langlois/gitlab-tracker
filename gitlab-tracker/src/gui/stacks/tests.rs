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

#[tokio::test]
async fn the_commands_move_each_mr_out_of_step_and_those_on_it() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let all = stacks(&app);
    let iid = crate::demo::iid;
    let of = |root: String| {
        let stack = all.iter().find(|s| s.root().id == root).expect(&root);
        commands(stack).expect("something to move")
    };
    // !209 lacks !198's last commits; !217 sits on !209, so it moves too,
    // onto the local !209 just moved. Pushed together, at the end.
    let notif = of(iid("push-notifications"));
    let rebase_209 = notif
        .find("git rebase --onto origin/feat/4302-push-notifications HEAD~")
        .expect(&notif);
    let rebase_217 = notif
        .find("git rebase --onto feat/4302-notification-preferences HEAD~")
        .expect(&notif);
    assert!(rebase_209 < rebase_217, "merge order:\n{notif}");
    assert!(notif.contains(
        "git push --force-with-lease origin feat/4302-notification-preferences feat/4302-mute-notifications"
    ));
    // !214 sits on !205, merged: onto main, then retargeted in GitLab.
    let pkce = of(iid("oauth-pkce"));
    assert!(
        pkce.contains("git switch feat/4188-pkce-verifier-store"),
        "{pkce}"
    );
    assert!(
        pkce.contains("git rebase --onto origin/main HEAD~"),
        "{pkce}"
    );
    assert!(
        pkce.contains(&format!("!{}", iid("pkce-verifier"))),
        "{pkce}"
    );
}

#[tokio::test]
async fn a_stack_in_step_has_no_commands() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let mut all = stacks(&app);
    for stack in &mut all {
        for node in &mut stack.nodes {
            node.problems.clear();
        }
        assert_eq!(commands(stack), None);
    }
}
