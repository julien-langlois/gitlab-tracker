use clap::{Parser, Subcommand};

use crate::storage::{load_projects_toml, resolve_active_project, ProjectEntry};

/// A fast terminal TUI dashboard for tracking GitLab Merge Requests across branches
#[derive(Parser, Debug)]
#[command(
    name = "gitlab-tracker",
    author,
    version,
    about,
    long_about = None
)]
pub struct Args {
    /// Launch in Demo Mode with mock data (for screenshots & testing)
    #[arg(long)]
    pub demo: bool,

    /// Select a project by its projects.toml name, GitLab project ID, or active index.
    #[arg(long, global = true)]
    pub project: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print tracker-specific status IDs that can be used in projects.toml mappings.
    TrackerStatuses,
    /// Print the tracked MRs as a Markdown release checklist (one column per branch).
    Checklist {
        /// Only the MRs of this milestone (exact title); untracked ones are fetched too.
        #[arg(long)]
        milestone: Option<String>,
        /// Only the MRs missing from this tracked branch (not merged, not cherry-picked).
        #[arg(long)]
        absent_from: Option<String>,
    },
    /// Import the tracker state of the versions before tracker.db (`tracker_<hash>.json`)
    /// into tracker.db: what the first launch does on its own. Every project of
    /// projects.toml, or the one given with --project.
    ImportState,
    /// Open the desktop window instead of the terminal UI.
    #[cfg(feature = "gui")]
    Gui,
}

/// Resolves the project requested by CLI flags or falls back to the active project.
pub async fn resolve_project(args: &Args) -> ProjectEntry {
    if let Some(selector) = args.project.as_deref() {
        let projects_cfg = load_projects_toml().await;
        let trimmed = selector.trim();
        let index = trimmed.parse::<usize>().ok();

        if let Some((_, project)) =
            projects_cfg
                .projects
                .into_iter()
                .enumerate()
                .find(|(idx, p)| {
                    index.is_some_and(|i| i == idx + 1)
                        || p.name.as_deref().is_some_and(|name| name == trimmed)
                        || p.project_id == trimmed
                })
        {
            return project;
        }

        eprintln!(
            "Project '{}' was not found in projects.toml; falling back to active project.",
            selector
        );
    }

    resolve_active_project().await
}

/// Runs a CLI subcommand when one was provided.
///
/// Returns `true` when the command was handled and the TUI should not start.
pub async fn run_command(
    command: Option<&Command>,
    project: &ProjectEntry,
    one_project: bool,
) -> anyhow::Result<bool> {
    match command {
        Some(Command::ImportState) => {
            import_state(project, one_project).await?;
            Ok(true)
        }
        Some(Command::TrackerStatuses) => {
            print_tracker_statuses(project).await?;
            Ok(true)
        }
        // Needs a full project session: run by `main`.
        Some(Command::Checklist { .. }) | None => Ok(false),
        #[cfg(feature = "gui")]
        Some(Command::Gui) => Ok(false),
    }
}

async fn import_state(project: &ProjectEntry, one_project: bool) -> anyhow::Result<()> {
    let projects = if one_project {
        vec![project.clone()]
    } else {
        load_projects_toml().await.projects
    };
    for p in &projects {
        let name = p.name.as_deref().unwrap_or(&p.project_id);
        match crate::storage::import_state_command(&p.gitlab_url, &p.project_id).await {
            Ok(Some(report)) => println!(
                "{name}: {} MRs imported, {} events of their history",
                report.mrs, report.events
            ),
            Ok(None) => println!("{name}: no state file to import"),
            Err(e) => println!("{name}: {e}"),
        }
    }
    Ok(())
}

async fn print_tracker_statuses(project: &ProjectEntry) -> anyhow::Result<()> {
    let Some(tracker_cfg) = project.tracker.as_ref() else {
        println!("No [project.tracker] section configured for the active project.");
        return Ok(());
    };

    #[cfg(feature = "redmine")]
    if tracker_cfg.provider.eq_ignore_ascii_case("redmine") {
        use anyhow::Context;
        use gitlab_tracker_core::TicketTransitionProvider;

        let (url, extra) = (tracker_cfg.url.clone(), tracker_cfg.extra.clone());
        // Keyring and prompt block: off the async runtime.
        let provider = tokio::task::spawn_blocking(move || {
            gitlab_tracker_redmine::RedmineProvider::from_tracker_section(&url, extra)
        })
        .await
        .context("Redmine setup task failed")??;
        let statuses = provider
            .fetch_transition_targets()
            .await
            .context("Failed to fetch Redmine issue statuses")?;

        if statuses.is_empty() {
            println!("No tracker statuses returned.");
            return Ok(());
        }

        println!("Tracker statuses for provider '{}':", tracker_cfg.provider);
        for status in statuses {
            println!("  {:>4}  {}", status.id, status.label);
        }

        return Ok(());
    }

    #[cfg(not(feature = "redmine"))]
    if tracker_cfg.provider.eq_ignore_ascii_case("redmine") {
        println!("Redmine support is not enabled. Rebuild with --features redmine.");
        return Ok(());
    }

    println!(
        "Tracker provider '{}' does not expose CLI status discovery yet.",
        tracker_cfg.provider
    );
    Ok(())
}
