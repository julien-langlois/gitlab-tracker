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
) -> Result<bool, Box<dyn std::error::Error>> {
    match command {
        Some(Command::TrackerStatuses) => {
            print_tracker_statuses(project).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn print_tracker_statuses(project: &ProjectEntry) -> Result<(), Box<dyn std::error::Error>> {
    let Some(tracker_cfg) = project.tracker.as_ref() else {
        println!("No [project.tracker] section configured for the active project.");
        return Ok(());
    };

    #[cfg(feature = "redmine")]
    if tracker_cfg.provider.eq_ignore_ascii_case("redmine") {
        use gitlab_tracker_core::TicketTransitionProvider;

        let (url, extra) = (tracker_cfg.url.clone(), tracker_cfg.extra.clone());
        // Keyring and prompt block: off the async runtime.
        let provider = tokio::task::spawn_blocking(move || {
            gitlab_tracker_redmine::RedmineProvider::from_tracker_section(&url, extra)
        })
        .await??;
        let statuses = provider.fetch_transition_targets().await?;

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
