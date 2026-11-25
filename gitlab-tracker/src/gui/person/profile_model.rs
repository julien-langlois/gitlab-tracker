//! A person's profile from the tracked MRs: weeks, recent MRs, who asks them for reviews, who could take some.

use super::*;

/// One week of someone's activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Week {
    /// Others' MRs they last commented on or approved that week.
    pub reviews: u32,
    /// Their MRs merged that week.
    pub merged: u32,
    /// Reviews last asked of them that week.
    pub asked: u32,
}

/// One of their MRs, the latest first.
#[derive(Debug, Clone)]
pub struct Recent {
    pub id: String,
    pub title: String,
    pub state: GitlabMrState,
    pub at: Option<DateTime<Utc>>,
}

/// What the tracked MRs tell about someone beyond what waits on them.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// Tracked MRs they appear in: author, reviewer, approver, commenter.
    pub seen: usize,
    /// Their MRs merged in the period, and in the period before.
    pub merged: u32,
    pub merged_before: u32,
    /// Median days from creation to merge: of their MRs, of the team's.
    pub cycle: Option<f64>,
    pub team_cycle: Option<f64>,
    /// Oldest first.
    pub weeks: [Week; WEEKS],
    /// Median days from the first review request to the first review, on
    /// their MRs first reviewed in the period.
    pub first_review: Option<f64>,
    /// Pushes after the first review, on average, on the same MRs.
    pub rounds: Option<f64>,
    /// Their MRs' pipelines started in the period: failed, finished.
    pub pipelines: (u32, u32),
    /// Median days of their MRs merged in the period: waiting for a review,
    /// from the first review to the approval, from the approval to the merge.
    pub stages: [Option<f64>; 3],
    /// Their open MRs and those opened in the period, by effort: easy,
    /// medium, complex.
    pub efforts: [u32; 3],
    /// The labels of the same MRs, the most used first.
    pub labels: Vec<(String, u32)>,
    /// Reviews asked of them in the period, those they approved, and the
    /// threads they opened on others' MRs.
    pub requested: u32,
    pub approved: u32,
    pub threads_opened: u32,
    /// The requests of the period answered within 4 h, 1 d, 3 d, later.
    pub buckets: [u32; 4],
    /// The requests of the period they have not answered.
    pub unanswered: u32,
    pub recent: Vec<Recent>,
}

/// Works out `who`'s profile from `app`'s MRs at `now`, over the last `days`.
pub fn profile(app: &App, who: &str, now: DateTime<Utc>, days: i64) -> Profile {
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let recent_enough = |t: Option<DateTime<Utc>>| t.is_some_and(|t| t >= start);
    let week_of = |t: DateTime<Utc>| -> Option<usize> {
        let ago = (now - t).num_days().div_euclid(7);
        (0..WEEKS as i64)
            .contains(&ago)
            .then(|| WEEKS - 1 - ago as usize)
    };
    let span = |from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>| -> Option<f64> {
        let (from, to) = (from?, to?);
        (to >= from).then(|| board::days_between(from, to))
    };
    let difficulty = &app.config.complexity_profile;
    let mut p = Profile::default();
    let mut cycles = Vec::new();
    let mut team_cycles = Vec::new();
    let mut first_reviews = Vec::new();
    let mut rounds = Vec::new();
    let mut stages: [Vec<f64>; 3] = Default::default();
    let mut labels: BTreeMap<String, u32> = BTreeMap::new();
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = login(&mr.author);
        let created = board::time_of(mr.created_at.as_deref());
        let merged = (mr.state == GitlabMrState::Merged)
            .then(|| board::time_of(mr.merged_at.as_deref()))
            .flatten();
        let cycle = span(created, merged);
        if recent_enough(merged) && !board::is_bot(author) {
            team_cycles.extend(cycle);
        }
        let review = mr.review.as_ref();
        let involved = author == who
            || mr.reviewers.iter().any(|r| login(r) == who)
            || mr.approved_by.iter().any(|a| a == who)
            || mr.threads.iter().any(|t| t.author == who)
            || review.is_some_and(|r| {
                r.last_activity_by.contains_key(who) || r.review_requested_at.contains_key(who)
            });
        if !involved {
            continue;
        }
        p.seen += 1;

        if author == who {
            // Up for review: out of draft, else asked; the opening for reads
            // without a request.
            let asked = review
                .and_then(|r| board::time_of(r.up_for_review_at()))
                .or(created);
            let first = review.and_then(|r| board::time_of(r.first_review_at.as_deref()));
            let approved = review.and_then(|r| board::time_of(r.approved_at.as_deref()));
            if let Some(at) = merged {
                if at >= start {
                    p.merged += 1;
                    cycles.extend(cycle);
                    stages[0].extend(span(asked, first));
                    stages[1].extend(span(first, approved));
                    stages[2].extend(span(approved, merged));
                } else if at >= before {
                    p.merged_before += 1;
                }
                if let Some(w) = week_of(at) {
                    p.weeks[w].merged += 1;
                }
            }
            if recent_enough(first) {
                first_reviews.extend(span(asked, first));
                rounds.push(f64::from(review.map_or(0, |r| r.pushes_after_review)));
            }
            for pipeline in &mr.pipelines {
                if !recent_enough(board::time_of(pipeline.created_at.as_deref())) {
                    continue;
                }
                match pipeline.status {
                    PipelineState::Failed => {
                        p.pipelines.0 += 1;
                        p.pipelines.1 += 1;
                    }
                    PipelineState::Success => p.pipelines.1 += 1,
                    _ => {}
                }
            }
            if mr.state == GitlabMrState::Opened || recent_enough(created) {
                if let Some(diff) = &mr.diff_stats {
                    let i = match Effort::from_score(diff.difficulty(difficulty)) {
                        Effort::Easy => 0,
                        Effort::Medium => 1,
                        Effort::Complex => 2,
                    };
                    p.efforts[i] += 1;
                }
                for label in &mr.labels {
                    *labels.entry(label.clone()).or_default() += 1;
                }
            }
            p.recent.push(Recent {
                id: mr.id.clone(),
                title: mr.title.clone(),
                state: mr.state,
                at: board::time_of(mr.updated_at.as_deref())
                    .or(merged)
                    .or(created),
            });
            continue;
        }

        // Someone else's MR: as a reviewer.
        if let Some(r) = review {
            let last = r
                .last_activity_by
                .get(who)
                .and_then(|at| board::parse_time(at));
            if let Some(w) = last.and_then(week_of) {
                p.weeks[w].reviews += 1;
            }
            if let Some(asked) = r
                .review_requested_at
                .get(who)
                .and_then(|at| board::parse_time(at))
            {
                if let Some(w) = week_of(asked) {
                    p.weeks[w].asked += 1;
                }
                if asked >= start {
                    p.requested += 1;
                    if mr.approved_by.iter().any(|a| a == who) {
                        p.approved += 1;
                    }
                    match span(Some(crate::utils::waits_from(r, asked)), last) {
                        Some(d) => {
                            let i = if d < QUICK {
                                0
                            } else if d < 1.0 {
                                1
                            } else if d < 3.0 {
                                2
                            } else {
                                3
                            };
                            p.buckets[i] += 1;
                        }
                        None => p.unanswered += 1,
                    }
                }
            }
        }
        p.threads_opened += mr
            .threads
            .iter()
            .filter(|t| t.author == who && recent_enough(board::parse_time(&t.created_at)))
            .count() as u32;
    }
    p.cycle = median(cycles);
    p.team_cycle = median(team_cycles);
    p.first_review = median(first_reviews);
    p.rounds = (!rounds.is_empty()).then(|| rounds.iter().sum::<f64>() / rounds.len() as f64);
    p.stages = stages.map(median);
    let mut labels: Vec<(String, u32)> = labels.into_iter().collect();
    labels.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    labels.truncate(4);
    p.labels = labels;
    p.recent.sort_by_key(|r| std::cmp::Reverse(r.at));
    p.recent.truncate(8);
    p
}

/// People by login, with a count.
pub(super) type Counts = Vec<(String, u32)>;

/// Who asks `who` for reviews (`asked_by`) and whom they ask (`asks`), over
/// the period, the most first.
pub(super) fn partners(analysis: &Analysis, who: &str) -> (Counts, Counts) {
    let mut asked_by: Counts = analysis
        .matrix
        .iter()
        .filter_map(|(author, row)| row.get(who).map(|n| (author.clone(), *n)))
        .filter(|(_, n)| *n > 0)
        .collect();
    let mut asks: Counts = analysis
        .matrix
        .get(who)
        .map(|row| row.iter().map(|(r, n)| (r.clone(), *n)).collect())
        .unwrap_or_default();
    for list in [&mut asked_by, &mut asks] {
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list.truncate(5);
    }
    (asked_by, asks)
}

/// When most of someone's requests go to `person` and `person` is loaded: who
/// could take some, preferring those who already reviewed that author.
pub(super) fn relief(analysis: &Analysis, person: &Person) -> Option<String> {
    if !matches!(person.level, Level::Overloaded | Level::Busy) {
        return None;
    }
    let (author, n, total) = analysis
        .matrix
        .iter()
        .filter_map(|(author, row)| {
            let n = *row.get(&person.login)?;
            Some((author, n, row.values().sum::<u32>()))
        })
        .filter(|(_, n, total)| *total >= 3 && f64::from(*n) / f64::from(*total) >= 0.5)
        .max_by_key(|(_, n, _)| *n)?;
    let name = |login: &str| -> String {
        analysis
            .people
            .iter()
            .find(|p| p.login == login)
            .map_or(login.to_string(), |p| p.name.clone())
    };
    let author_name = name(author);
    let share = board::percent(f64::from(n) / f64::from(total));
    let candidates = analysis.people.iter().filter(|p| {
        p.login != person.login
            && p.login != *author
            && matches!(p.level, Level::Available | Level::Normal)
            && p.load < person.load
    });
    let reviewed = |p: &&Person| {
        analysis
            .matrix
            .get(author)
            .and_then(|row| row.get(&p.login))
            .is_some_and(|n| *n > 0)
    };
    let lightest = |a: &&Person, b: &&Person| a.load.total_cmp(&b.load);
    let known = candidates.clone().filter(reviewed).min_by(lightest);
    let mut text = tf(
        "{share} of {author}'s review requests go to {name}.",
        &[
            ("share", &share),
            ("author", board::first_name(&author_name)),
            ("name", board::first_name(&person.name)),
        ],
    );
    let other = match known {
        Some(p) => tf(
            "{other} already reviews {author} and is less loaded: they could take some.",
            &[
                ("other", board::first_name(&p.name)),
                ("author", board::first_name(&author_name)),
            ],
        ),
        None => match candidates.min_by(lightest) {
            Some(p) => tf(
                "{other} is less loaded and could take some.",
                &[("other", board::first_name(&p.name))],
            ),
            None => String::new(),
        },
    };
    if !other.is_empty() {
        text.push(' ');
        text.push_str(&other);
    }
    Some(text)
}
