//! The environment branches in flow order, what each holds that the next lacks, and the open MRs.

use super::*;

/// A tracked branch, as the page shows it.
pub(super) struct Branch {
    pub(super) name: String,
    pub(super) alias: String,
    pub(super) role: BranchRole,
    /// The environments deployed from it, the latest deployment first.
    pub(super) environments: Vec<String>,
    pub(super) last: Option<DeploymentRecord>,
    pub(super) pipeline: Option<BranchPipeline>,
    /// The version it runs: a release's tag, and the MRs on it merged since.
    pub(super) version: Option<(String, usize)>,
}

/// An MR on a branch and not on another: id, title, since when on the first.
pub(super) type Waiting = (String, String, Option<DateTime<Utc>>);

/// What `app` has on each pair of branches, and the branches in flow order.
pub(in crate::gui) struct Model {
    pub(super) branches: Vec<Branch>,
    /// (from, to) → the MRs on `from` not (all) on `to`, with since when.
    pub(super) gaps: HashMap<(usize, usize), Vec<Waiting>>,
    pub(super) open: Vec<OpenMr>,
}

pub(super) struct OpenMr {
    pub(super) blocked: bool,
    pub(super) ready: bool,
}

pub(super) fn role_color(role: BranchRole, theme: &Theme) -> Color32 {
    let t = &theme.tones;
    match role {
        BranchRole::Feature => t.gray.fg,
        BranchRole::Integration => t.blue.fg,
        BranchRole::Staging => t.purple.fg,
        BranchRole::Production => t.green.fg,
    }
}

pub(super) fn model(app: &App, overview: Option<&ProjectOverview>) -> Model {
    let flow = app.branch_flow();
    let ordered = flow.in_order();
    let branches: Vec<Branch> = ordered
        .iter()
        .map(|b| {
            let deployments: Vec<&DeploymentRecord> = overview
                .map(|o| {
                    o.deployments
                        .iter()
                        .filter(|d| d.reference == b.name)
                        .collect()
                })
                .unwrap_or_default();
            let mut environments: Vec<String> = Vec::new();
            for d in &deployments {
                if !environments.contains(&d.environment) {
                    environments.push(d.environment.clone());
                }
            }
            let last = deployments
                .iter()
                .find(|d| d.status == "success")
                .map(|d| (*d).clone());
            Branch {
                version: overview.and_then(|o| version_of(app, o, &b.name, last.as_ref())),
                name: b.name.clone(),
                alias: b.alias.clone(),
                role: b.role,
                environments,
                last,
                pipeline: overview
                    .and_then(|o| o.pipelines.iter().find(|p| p.branch == b.name).cloned()),
            }
        })
        .collect();
    let mut gaps = HashMap::new();
    for (i, from) in branches.iter().enumerate() {
        for (j, to) in branches.iter().enumerate() {
            if i == j {
                continue;
            }
            let list: Vec<Waiting> = promote::rows(app, &from.name, &to.name)
                .into_iter()
                .filter(|r| r.stage == Stage::Ready)
                .map(|r| (r.id, r.title, r.on_from_since))
                .collect();
            gaps.insert((i, j), list);
        }
    }
    let open = app
        .mrs
        .iter()
        .filter(|mr| mr.state == GitlabMrState::Opened && !mr.title.is_empty())
        .map(|mr| {
            let failed = mr
                .pipelines
                .first()
                .is_some_and(|p| p.status == PipelineState::Failed);
            let blocked = failed
                || matches!(
                    mr.mergeability,
                    MergeabilityStatus::Conflict
                        | MergeabilityStatus::RequestedChanges
                        | MergeabilityStatus::CiMustPass
                );
            OpenMr {
                blocked,
                ready: !blocked
                    && mr.mergeability == MergeabilityStatus::Mergeable
                    && !mr.approved_by.is_empty(),
            }
        })
        .collect();
    Model {
        branches,
        gaps,
        open,
    }
}

impl Model {
    pub(super) fn gap(&self, from: usize, to: usize) -> &[Waiting] {
        self.gaps.get(&(from, to)).map_or(&[], Vec::as_slice)
    }

    pub(super) fn index(&self, name: &str) -> Option<usize> {
        self.branches.iter().position(|b| b.name == name)
    }

    /// The pair to show: the one picked, else the consecutive pair with the
    /// most waiting, else the last hop.
    pub(super) fn pair(&self, picked: Option<&(String, String)>) -> Option<(usize, usize)> {
        if let Some((from, to)) = picked {
            if let (Some(i), Some(j)) = (self.index(from), self.index(to)) {
                return Some((i, j));
            }
        }
        let n = self.branches.len();
        if n < 2 {
            return None;
        }
        (0..n - 1)
            .max_by_key(|i| (self.gap(*i, i + 1).len(), *i))
            .map(|i| (i, i + 1))
    }
}

/// The version `branch` runs: the release whose commit its last deployment
/// is, exactly; else the latest release and the MRs that reached the branch
/// before that deployment and are not on production yet (the last tracked
/// branch, where releases are made). `None` without a release, or without a
/// deployment.
pub(super) fn version_of(
    app: &App,
    overview: &ProjectOverview,
    branch: &str,
    last: Option<&DeploymentRecord>,
) -> Option<(String, usize)> {
    let deployed = last?;
    let same = |sha: &str| {
        !sha.is_empty()
            && !deployed.sha.is_empty()
            && (sha.starts_with(&deployed.sha) || deployed.sha.starts_with(sha))
    };
    if let Some(r) = overview.releases.iter().find(|r| same(&r.sha)) {
        return Some((r.tag.clone(), 0));
    }
    let latest = overview.releases.first()?;
    let production = app.branch_flow().in_order().last()?.name.clone();
    let on = |mr: &crate::models::TrackedMr, b: &str| {
        app.branch_presence(mr, b).is_some_and(|p| !p.is_missing())
    };
    // What the deployment holds: the MRs that reached the branch by then
    // (when their arrival is known).
    let deployed_at = board::parse_time(&deployed.created_at);
    let since = app
        .mrs
        .iter()
        .filter(|mr| mr.state == GitlabMrState::Merged)
        .filter(|mr| on(mr, branch) && !on(mr, &production))
        .filter(|mr| {
            match (
                super::super::milestones::arrived(app, mr, branch),
                deployed_at,
            ) {
                (Some(arrived), Some(deployed)) => arrived <= deployed,
                _ => true,
            }
        })
        .count();
    Some((latest.tag.clone(), since))
}

pub(super) fn oldest(list: &[Waiting]) -> Option<DateTime<Utc>> {
    list.iter().filter_map(|r| r.2).min()
}
