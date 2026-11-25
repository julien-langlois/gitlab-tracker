//! Reading the tracked MRs into each person's load: what waits on them, whom they help.

use super::*;

/// How loaded someone is, from their [`Person::load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Overloaded,
    Busy,
    Normal,
    Available,
    Bot,
}

impl Level {
    pub(in crate::gui) fn label(self) -> &'static str {
        tl(match self {
            Level::Overloaded => "Overloaded",
            Level::Busy => "Busy",
            Level::Normal => "Normal",
            Level::Available => "Available",
            Level::Bot => "Bot",
        })
    }

    pub(in crate::gui) fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Level::Overloaded => t.red,
            Level::Busy => t.amber,
            Level::Normal => t.blue,
            Level::Available => t.green,
            Level::Bot => t.gray,
        }
    }
}

/// A review waiting on someone.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub id: String,
    pub title: String,
    pub author: String,
    pub since: Option<DateTime<Utc>>,
}

/// Someone else's open MR a person pushed commits to.
#[derive(Debug, Clone)]
pub struct Helped {
    pub id: String,
    pub title: String,
    pub author: String,
    /// Their last push to it.
    pub at: DateTime<Utc>,
}

/// One of someone's open MRs and its next step.
#[derive(Debug, Clone)]
pub struct Own {
    pub id: String,
    pub title: String,
    pub step: NextStep,
    pub effort: Option<Effort>,
}

/// A person of the team, as the tracked MRs show them.
#[derive(Debug, Clone)]
pub struct Person {
    pub login: String,
    pub name: String,
    pub reviews: Vec<Waiting>,
    pub mrs: Vec<Own>,
    /// Others' open MRs they pushed to, latest push first: who knows them
    /// already, should the author be away.
    pub helped: Vec<Helped>,
    /// Unresolved threads on their MRs whose turn is theirs.
    pub threads: usize,
    /// Median days from a review request to their next activity on the MR.
    pub response: Option<f64>,
    /// 0 to 1, against the most loaded person.
    pub load: f32,
    pub level: Level,
    /// Reviews they took part in, per week, oldest first (8 weeks).
    pub weeks: [u32; 8],
    /// When they were last asked for a review.
    pub last_request: Option<DateTime<Utc>>,
}

impl Person {
    /// Someone the tracked MRs say nothing about yet.
    pub fn blank(login: &str) -> Self {
        Self {
            login: login.to_string(),
            name: String::new(),
            reviews: Vec::new(),
            mrs: Vec::new(),
            helped: Vec::new(),
            threads: 0,
            response: None,
            load: 0.0,
            level: Level::Normal,
            weeks: [0; 8],
            last_request: None,
        }
    }

    /// The oldest review waiting on them.
    pub(in crate::gui) fn oldest(&self) -> Option<DateTime<Utc>> {
        self.reviews.iter().filter_map(|r| r.since).min()
    }
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub people: Vec<Person>,
    /// Review requests in the period: author login → reviewer login → count.
    pub matrix: BTreeMap<String, BTreeMap<String, u32>>,
    /// Median response over the period, and over the period before.
    pub response: Option<f64>,
    pub response_before: Option<f64>,
    pub open: usize,
}

/// The display name of a "Full Name (@login)" string.
pub(super) fn name_of(display: &str) -> String {
    split_person(display).0.to_string()
}

/// Works the team out of `app`'s MRs at `now`, over the last `days`.
pub fn analyze(app: &App, now: DateTime<Utc>, days: i64, bots: bool) -> Analysis {
    let me = app.config.gitlab_username.as_deref();
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let mut names: HashMap<String, String> = HashMap::new();
    let mut remember = |display: &str| {
        let key = login(display).to_string();
        if key.is_empty() {
            return key;
        }
        let entry = names.entry(key.clone()).or_default();
        if entry.is_empty() || (display.contains("(@") && !entry.contains(' ')) {
            *entry = name_of(display);
        }
        key
    };
    let mut people: BTreeMap<String, Person> = BTreeMap::new();
    let blank = Person::blank;
    let mut responses: HashMap<String, Vec<f64>> = HashMap::new();
    let mut now_period = Vec::new();
    let mut before_period = Vec::new();
    let mut matrix: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut open = 0;
    let profile = &app.config.complexity_profile;
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let author = remember(&mr.author);
        let reviewers: Vec<String> = mr.reviewers.iter().map(|r| remember(r)).collect();
        for approver in &mr.approvers {
            remember(approver);
        }
        if mr.state == GitlabMrState::Opened {
            open += 1;
            if !author.is_empty() {
                let step = next_step::of(app, mr, me);
                let effort = mr
                    .diff_stats
                    .as_ref()
                    .map(|d| Effort::from_score(d.difficulty(profile)));
                let threads = mr
                    .threads
                    .iter()
                    .filter(|t| !t.resolved && next_step::thread_turn(mr, t) == Turn::Author)
                    .count();
                let person = people
                    .entry(author.clone())
                    .or_insert_with(|| blank(&author));
                person.threads += threads;
                person.mrs.push(Own {
                    id: mr.id.clone(),
                    title: mr.title.clone(),
                    step,
                    effort,
                });
            }
            for reviewer in &reviewers {
                if reviewer.is_empty() || *reviewer == author {
                    continue;
                }
                let waiting = crate::utils::awaiting_review_by(
                    mr.state,
                    mr.mergeability,
                    &mr.reviewers,
                    &mr.approved_by,
                    reviewer,
                );
                let person = people
                    .entry(reviewer.clone())
                    .or_insert_with(|| blank(reviewer));
                if waiting {
                    person.reviews.push(Waiting {
                        id: mr.id.clone(),
                        title: mr.title.clone(),
                        author: mr.author.clone(),
                        since: crate::utils::review_waiting_since(
                            mr.review.as_ref(),
                            mr.created_at.as_deref(),
                            reviewer,
                        ),
                    });
                }
            }
        }
        // The review history: requests, answers, activity by week.
        let Some(review) = mr.review.as_ref() else {
            continue;
        };
        for (reviewer, requested) in &review.review_requested_at {
            let Some(requested) = board::parse_time(requested) else {
                continue;
            };
            if *reviewer == author {
                continue;
            }
            let person = people
                .entry(reviewer.clone())
                .or_insert_with(|| blank(reviewer));
            person.last_request = person.last_request.max(Some(requested));
            let bot = board::is_bot(&author) || board::is_bot(reviewer);
            if requested >= start && !author.is_empty() && (bots || !bot) {
                *matrix
                    .entry(author.clone())
                    .or_default()
                    .entry(reviewer.clone())
                    .or_default() += 1;
            }
            // A comment, an approval or a push answers the request: the
            // first of them since it.
            let answered = [&review.last_activity_by, &review.last_push_by]
                .into_iter()
                .filter_map(|by| by.get(reviewer).and_then(|at| board::parse_time(at)))
                .filter(|at| *at >= requested)
                .min();
            if let Some(at) = answered {
                // Asked on a draft: the answer is waited for once it is ready.
                let days =
                    board::days_between(crate::utils::waits_from(review, requested), at).max(0.0);
                if requested >= start {
                    responses.entry(reviewer.clone()).or_default().push(days);
                    now_period.push(days);
                } else if requested >= before {
                    before_period.push(days);
                }
            }
        }
        // Pushes to someone else's MR are part of its review: one per MR and
        // person, at the latest of their comments, approvals and pushes.
        let mut took_part: BTreeMap<&String, DateTime<Utc>> = BTreeMap::new();
        for (who, at) in review.last_activity_by.iter().chain(&review.last_push_by) {
            if let Some(at) = board::parse_time(at) {
                let latest = took_part.entry(who).or_insert(at);
                *latest = (*latest).max(at);
            }
        }
        if mr.state == GitlabMrState::Opened {
            for (who, at) in &review.last_push_by {
                if *who == author || who.is_empty() || (!bots && board::is_bot(who)) {
                    continue;
                }
                let Some(at) = board::parse_time(at) else {
                    continue;
                };
                let person = people.entry(who.clone()).or_insert_with(|| blank(who));
                person.helped.push(Helped {
                    id: mr.id.clone(),
                    title: mr.title.clone(),
                    author: mr.author.clone(),
                    at,
                });
            }
        }
        for (who, at) in took_part {
            if *who == author {
                continue;
            }
            let weeks_ago = (now - at).num_days() / 7;
            if (0..8).contains(&weeks_ago) {
                let person = people.entry(who.clone()).or_insert_with(|| blank(who));
                person.weeks[7 - weeks_ago as usize] += 1;
            }
        }
    }

    for person in people.values_mut() {
        person.helped.sort_by_key(|h| std::cmp::Reverse(h.at));
    }

    // The load: reviews weighted by how long they wait, own MRs by their effort,
    // threads to answer. Relative to the most loaded person.
    let raw = |p: &Person| -> f32 {
        let reviews: f32 = p
            .reviews
            .iter()
            .map(|r| {
                let age = r.since.map_or(0.0, |s| board::days_between(s, now) as f32);
                1.0 + age.clamp(0.0, 5.0) / 2.5
            })
            .sum();
        let own: f32 = p
            .mrs
            .iter()
            .map(|m| match m.effort {
                Some(Effort::Complex) => 1.5,
                Some(Effort::Medium) => 1.0,
                _ => 0.5,
            })
            .sum();
        reviews + own + p.threads as f32 * 0.3
    };
    let mut list: Vec<Person> = people
        .into_values()
        .filter(|p| !p.login.is_empty())
        .map(|mut p| {
            p.name = names
                .get(&p.login)
                .cloned()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| p.login.clone());
            p.response = median(responses.remove(&p.login).unwrap_or_default());
            p
        })
        .filter(|p| bots || !(board::is_bot(&p.login) || board::is_bot(&p.name)))
        // Someone the tracked MRs say nothing recent about stays out.
        .filter(|p| {
            !p.reviews.is_empty()
                || !p.mrs.is_empty()
                || p.weeks.iter().any(|w| *w > 0)
                || p.last_request.is_some_and(|r| r >= start)
        })
        .collect();
    let most = list
        .iter()
        .filter(|p| !board::is_bot(&p.login))
        .map(&raw)
        .fold(0.0_f32, f32::max);
    for p in &mut list {
        let value = raw(p);
        p.load = if most > 0.0 {
            (value / most).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.level = if board::is_bot(&p.login) || board::is_bot(&p.name) {
            Level::Bot
        } else if p.load >= 0.8 && value >= 3.0 {
            Level::Overloaded
        } else if p.load >= 0.55 && value >= 2.0 {
            Level::Busy
        } else if p.reviews.is_empty() && p.load < 0.3 {
            Level::Available
        } else {
            Level::Normal
        };
    }
    Analysis {
        people: list,
        matrix,
        response: median(now_period),
        response_before: median(before_period),
        open,
    }
}

/// The reviewers' share of the requests: the busiest's login and share.
pub(in crate::gui) fn busiest(
    matrix: &BTreeMap<String, BTreeMap<String, u32>>,
) -> Option<(String, f64)> {
    let mut by_reviewer: BTreeMap<&str, u32> = BTreeMap::new();
    for row in matrix.values() {
        for (reviewer, n) in row {
            *by_reviewer.entry(reviewer).or_default() += n;
        }
    }
    let total: u32 = by_reviewer.values().sum();
    let (who, n) = by_reviewer.iter().max_by_key(|(_, n)| **n)?;
    (total > 0).then(|| (who.to_string(), f64::from(*n) / f64::from(total)))
}
