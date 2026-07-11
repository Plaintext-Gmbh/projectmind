// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Auto-narrated tour scaffolding (#201, V4.4).
//!
//! ProjectMind is an MCP *server*; the LLM is the *client*. There is no
//! LLM-call path in this repo and this module does not add one. Instead it
//! composes the existing importance heuristics into a machine-readable
//! *scaffold*: a ranked list of the modules worth touring plus ready-made
//! step targets. The client (Claude) turns the `facts` bullets into prose
//! and drives the existing `walkthrough_start`; a server-side `materialize`
//! mode (in the MCP layer) fills the narration from a template so an
//! offline / TTS demo works without any client LLM.
//!
//! The ranking is a pure, deterministic function of three signals that are
//! already computed elsewhere:
//!
//! - **coupling** — a module's cross-module edges (`incoming + outgoing`)
//!   from [`crate::module_chord`]. Central modules earn a tour stop.
//! - **size** — the module's class count. Bigger modules carry more of the
//!   story.
//! - **activity** — commits in the trailing 90-day window from
//!   [`crate::git::commit_activity`]. Hot modules are where change happens.
//!
//! Each signal is min-max normalised across the modules, then combined with
//! the weights in [`TourWeights`] (default `0.4 / 0.3 / 0.3`). The top class
//! of each module is the highest-`fan_in` class from [`crate::risk::compute`]
//! — the one the most other classes depend on.

use projectmind_plugin_api::FrameworkPlugin;
use serde::Serialize;

use crate::git::{self, CommitActivity};
use crate::module_chord::{self, ModuleChord};
use crate::repository::Repository;
use crate::risk::{self, Options as RiskOptions, RiskScore};

/// Commits inside this trailing window count toward the "activity" signal.
pub const ACTIVITY_WINDOW_DAYS: u64 = 90;

/// Weight applied to the (normalised) cross-module coupling signal.
pub const DEFAULT_WEIGHT_COUPLING: f64 = 0.4;
/// Weight applied to the (normalised) class-count signal.
pub const DEFAULT_WEIGHT_SIZE: f64 = 0.3;
/// Weight applied to the (normalised) 90-day commit-activity signal.
pub const DEFAULT_WEIGHT_ACTIVITY: f64 = 0.3;

/// Weights blending the three module-ranking signals into a score.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TourWeights {
    /// Weight on cross-module coupling (`incoming + outgoing`).
    pub coupling: f64,
    /// Weight on class count.
    pub size: f64,
    /// Weight on 90-day commit activity.
    pub activity: f64,
}

impl Default for TourWeights {
    fn default() -> Self {
        Self {
            coupling: DEFAULT_WEIGHT_COUPLING,
            size: DEFAULT_WEIGHT_SIZE,
            activity: DEFAULT_WEIGHT_ACTIVITY,
        }
    }
}

/// Which audience the tour is pitched at. Only affects framing text today;
/// the ranking is identical so the scaffold stays deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Persona {
    /// Someone new to the codebase — the default. Leads with orientation.
    NewDev,
    /// An architect — leads with coupling / hotspots.
    Architect,
}

impl Persona {
    /// Parse the wire value. Unknown strings fall back to [`Persona::NewDev`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "architect" => Self::Architect,
            _ => Self::NewDev,
        }
    }

    /// A one-line framing sentence for the opening `Overview` step.
    const fn overview_hint(self) -> &'static str {
        match self {
            Self::NewDev => "Start here to get oriented before reading any source.",
            Self::Architect => {
                "The most-coupled and busiest modules first — where the design lives."
            }
        }
    }
}

/// The top class of a module, with the numbers that made it the pick.
#[derive(Debug, Clone, Serialize)]
pub struct TopClass {
    /// Fully-qualified class name.
    pub fqn: String,
    /// Composite 0..=100 risk score (from [`crate::risk`]).
    pub risk_score: f64,
    /// How many other classes reference it (fan-in) — the reason it's the pick.
    pub fan_in: u32,
    /// Short human-readable hint (`why` from the risk atlas, e.g. `hot+central`).
    pub why: String,
}

/// One ranked module in the tour.
#[derive(Debug, Clone, Serialize)]
pub struct RankedModule {
    /// Module id (matches `repo.modules` key).
    pub module: String,
    /// Blended 0..=1 ranking score (higher = more tour-worthy).
    pub score: f64,
    /// Number of classes parsed for the module.
    pub classes: usize,
    /// Inbound cross-module edges.
    pub fan_in: usize,
    /// Outbound cross-module edges.
    pub fan_out: usize,
    /// Commits touching the module in the last 90 days.
    pub commits_90d: usize,
    /// The module's most-depended-on class, or `None` when it has no classes.
    pub top_class: Option<TopClass>,
    /// Human-readable bullets the client can narrate verbatim or paraphrase.
    pub facts: Vec<String>,
}

/// The target of a suggested step, mirroring the [`crate::walkthrough`]
/// target kinds the viewers already understand. Kept as a small local enum
/// so `tour_suggest` stays free of walkthrough IO; the MCP layer maps these
/// onto real `WalkthroughTarget`s when materialising.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StepTarget {
    /// Narration-only intro (`WalkthroughTarget::Note`).
    Note,
    /// The Risk Atlas treemap, optionally ringing named hotspots.
    Atlas {
        /// Fully-qualified class names to ring as hotspots.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        highlight: Vec<String>,
    },
    /// A single class in the open repo (`WalkthroughTarget::Class`).
    Class {
        /// Fully-qualified class name to open.
        fqn: String,
    },
}

/// One suggested tour step: a title, a ready-made target, and the facts the
/// client should weave into narration.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedStep {
    /// Short step title (sidebar entry).
    pub title: String,
    /// Where the step points.
    pub target: StepTarget,
    /// Facts to narrate. Empty is legal (a pure framing step).
    pub facts: Vec<String>,
}

/// Repository-level headline numbers for the tour intro.
#[derive(Debug, Clone, Serialize)]
pub struct RepoHead {
    /// Display title (the repo directory name).
    pub title: String,
    /// Total module count.
    pub modules_total: usize,
    /// Total class count.
    pub classes_total: usize,
}

/// The full scaffold returned to the caller.
#[derive(Debug, Clone, Serialize)]
pub struct TourScaffold {
    /// Repository headline numbers.
    pub repo: RepoHead,
    /// Ranked modules, highest score first, capped at the requested `top`.
    pub ranking: Vec<RankedModule>,
    /// Ready-made steps: an overview, one per ranked module, and a closing
    /// "where change happens" atlas step.
    pub suggested_steps: Vec<SuggestedStep>,
}

/// Build a tour scaffold for `repo` using `framework` for relations.
///
/// `top` caps the number of ranked modules (clamped to at least 1). `persona`
/// only tweaks framing text, keeping the ranking deterministic. Reads git
/// history for the activity signal; an empty or non-git repo degrades to a
/// zero-activity ranking rather than erroring.
///
/// An empty repo yields an empty `ranking` and a lone `Overview` step — never
/// an error.
#[must_use]
pub fn suggest_tour(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
    top: usize,
    persona: Persona,
) -> TourScaffold {
    suggest_tour_with(repo, framework, top, persona, TourWeights::default())
}

/// [`suggest_tour`] with explicit weights — the seam the unit tests drive.
#[must_use]
pub fn suggest_tour_with(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
    top: usize,
    persona: Persona,
    weights: TourWeights,
) -> TourScaffold {
    let top = top.max(1);
    let chord = module_chord::build(repo, framework);
    let activity = git::commit_activity(&repo.root);
    let relations = collect_relations(repo, framework);
    let top_classes = top_class_per_module(repo, &relations);

    let ranked = rank_modules(&chord, &activity, &top_classes, weights, top);
    let steps = build_steps(&ranked, persona);

    TourScaffold {
        repo: RepoHead {
            title: repo_title(repo),
            modules_total: repo.modules.len(),
            classes_total: repo.class_count(),
        },
        ranking: ranked,
        suggested_steps: steps,
    }
}

/// Flatten every framework relation across the repo (same shape the risk
/// atlas feeds on). Kept local so core need not depend on the engine.
fn collect_relations(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> Vec<projectmind_plugin_api::Relation> {
    let mut out = Vec::new();
    for module in repo.modules.values() {
        out.extend(framework.relations(module));
    }
    out
}

/// Compute risk once over the whole repo, then keep the highest-`fan_in`
/// class per module. One git-churn walk instead of one per module.
fn top_class_per_module(
    repo: &Repository,
    relations: &[projectmind_plugin_api::Relation],
) -> std::collections::HashMap<String, TopClass> {
    let opts = RiskOptions {
        module: None,
        // Ask for every class so no module drops off the tail.
        top: repo.class_count().max(1),
        window_days: risk::DEFAULT_CHURN_WINDOW_DAYS,
        weights: risk::Weights::default(),
    };
    // Coverage is optional and orthogonal to fan-in selection; skip it here so
    // the scaffold never depends on a test run having happened. A non-git repo
    // (or any risk error) degrades to "no top classes" rather than failing.
    let Ok(scores) = risk::compute(repo, relations, None, &opts) else {
        return std::collections::HashMap::new();
    };

    let mut best: std::collections::HashMap<String, RiskScore> = std::collections::HashMap::new();
    for s in scores {
        best.entry(s.module.clone())
            .and_modify(|cur| {
                if is_better_top(&s, cur) {
                    *cur = s.clone();
                }
            })
            .or_insert(s);
    }

    best.into_iter()
        .map(|(module, s)| {
            (
                module,
                TopClass {
                    fqn: s.fqn,
                    risk_score: s.score,
                    fan_in: s.fan_in,
                    why: s.why,
                },
            )
        })
        .collect()
}

/// A class beats the current pick when it has a higher fan-in, or an equal
/// fan-in with a higher risk score. Ties break on fqn for determinism.
fn is_better_top(candidate: &RiskScore, current: &RiskScore) -> bool {
    match candidate.fan_in.cmp(&current.fan_in) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match candidate
            .score
            .partial_cmp(&current.score)
            .unwrap_or(std::cmp::Ordering::Equal)
        {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => candidate.fqn < current.fqn,
        },
    }
}

/// Count commits within the 90-day window for `module` from the (24-month)
/// activity payload.
fn commits_90d_for(activity: &CommitActivity, module: &str) -> usize {
    let cutoff = ACTIVITY_WINDOW_DAYS * 86_400;
    activity
        .modules
        .iter()
        .find(|m| m.module == module)
        .map_or(0, |m| {
            m.commits.iter().filter(|c| c.secs_ago <= cutoff).count()
        })
}

/// Min-max normalise `v` into 0..=1 given the observed `max`. A zero max
/// (every module equal / empty) maps everything to 0 so the signal simply
/// doesn't move the ranking.
fn norm(v: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        v / max
    }
}

/// Score and sort the modules, capping at `top`.
fn rank_modules(
    chord: &ModuleChord,
    activity: &CommitActivity,
    top_classes: &std::collections::HashMap<String, TopClass>,
    weights: TourWeights,
    top: usize,
) -> Vec<RankedModule> {
    struct Row {
        module: String,
        classes: usize,
        fan_in: usize,
        fan_out: usize,
        coupling: usize,
        commits_90d: usize,
    }

    let mut rows: Vec<Row> = chord
        .modules
        .iter()
        .map(|m| {
            let commits_90d = commits_90d_for(activity, &m.id);
            Row {
                module: m.id.clone(),
                classes: m.classes,
                fan_in: m.incoming,
                fan_out: m.outgoing,
                coupling: m.incoming + m.outgoing,
                commits_90d,
            }
        })
        .collect();

    let max_coupling = rows.iter().map(|r| r.coupling).max().unwrap_or(0) as f64;
    let max_classes = rows.iter().map(|r| r.classes).max().unwrap_or(0) as f64;
    let max_commits = rows.iter().map(|r| r.commits_90d).max().unwrap_or(0) as f64;

    let mut ranked: Vec<RankedModule> = rows
        .drain(..)
        .map(|r| {
            let score = weights.coupling * norm(r.coupling as f64, max_coupling)
                + weights.size * norm(r.classes as f64, max_classes)
                + weights.activity * norm(r.commits_90d as f64, max_commits);
            let top_class = top_classes.get(&r.module).cloned();
            let facts = module_facts(
                &r.module,
                r.fan_in,
                r.commits_90d,
                r.classes,
                top_class.as_ref(),
            );
            RankedModule {
                module: r.module,
                score: (score * 1000.0).round() / 1000.0,
                classes: r.classes,
                fan_in: r.fan_in,
                fan_out: r.fan_out,
                commits_90d: r.commits_90d,
                top_class,
                facts,
            }
        })
        .collect();

    // Highest score first; ties break on module id for a stable order.
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.module.cmp(&b.module))
    });
    ranked.truncate(top);
    ranked
}

/// Human-readable bullets for one module.
fn module_facts(
    module: &str,
    fan_in: usize,
    commits_90d: usize,
    classes: usize,
    top_class: Option<&TopClass>,
) -> Vec<String> {
    let mut facts = vec![format!(
        "`{module}` has {classes} class(es), {fan_in} module(s) depend on it, {commits_90d} commit(s) in 90d"
    )];
    if fan_in > 0 {
        facts.push(format!("depended on by {fan_in} module(s) (fan-in)"));
    }
    if commits_90d > 0 {
        facts.push(format!("busy: {commits_90d} commit(s) in the last 90 days"));
    }
    if let Some(tc) = top_class {
        facts.push(format!(
            "top class `{}` (risk {:.0}, {} class(es) depend on it, {})",
            tc.fqn, tc.risk_score, tc.fan_in, tc.why
        ));
    }
    facts
}

/// Build the suggested-step list: Overview → one Class step per ranked module
/// → a closing "Where change happens" atlas step.
fn build_steps(ranked: &[RankedModule], persona: Persona) -> Vec<SuggestedStep> {
    let mut steps = Vec::with_capacity(ranked.len() + 2);

    // Overview — narration-only orientation.
    let mut overview_facts = vec![persona.overview_hint().to_string()];
    if let Some(first) = ranked.first() {
        overview_facts.push(format!(
            "start with `{}`, the highest-ranked module",
            first.module
        ));
    }
    steps.push(SuggestedStep {
        title: "Overview".to_string(),
        target: StepTarget::Note,
        facts: overview_facts,
    });

    // One step per ranked module, pointing at its top class when it has one.
    for m in ranked {
        if let Some(tc) = &m.top_class {
            steps.push(SuggestedStep {
                title: m.module.clone(),
                target: StepTarget::Class {
                    fqn: tc.fqn.clone(),
                },
                facts: m.facts.clone(),
            });
        } else {
            steps.push(SuggestedStep {
                title: m.module.clone(),
                target: StepTarget::Note,
                facts: m.facts.clone(),
            });
        }
    }

    // Closing step — the hotspots ringed on the Risk Atlas.
    let hotspots: Vec<String> = ranked
        .iter()
        .filter_map(|m| m.top_class.as_ref().map(|tc| tc.fqn.clone()))
        .collect();
    let mut change_facts =
        vec!["these are the highest-risk classes across the toured modules".to_string()];
    if !hotspots.is_empty() {
        change_facts.push(format!("{} hotspot class(es) ringed", hotspots.len()));
    }
    steps.push(SuggestedStep {
        title: "Where change happens".to_string(),
        target: StepTarget::Atlas {
            highlight: hotspots,
        },
        facts: change_facts,
    });

    steps
}

/// Repo display title = the root directory's file name, falling back to the
/// full path string when there's no file component.
fn repo_title(repo: &Repository) -> String {
    repo.root.file_name().map_or_else(
        || repo.root.to_string_lossy().into_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{
        Annotation, Class, Module, PluginInfo, Relation, RelationKind, Result as PluginResult,
    };
    use std::path::PathBuf;

    /// Minimal framework that echoes a fixed relation set per module.
    struct DummyFw {
        relations: Vec<(String, Vec<Relation>)>,
    }
    impl std::fmt::Debug for DummyFw {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("DummyFw")
        }
    }
    impl FrameworkPlugin for DummyFw {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "t",
                name: "t",
                version: "0",
            }
        }
        fn supported_languages(&self) -> &[&'static str] {
            &["lang-test"]
        }
        fn enrich(&self, _: &mut Module) -> PluginResult<()> {
            Ok(())
        }
        fn relations(&self, module: &Module) -> Vec<Relation> {
            self.relations
                .iter()
                .find(|(m, _)| m == &module.id)
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        }
        fn provided_diagrams(&self) -> &[&'static str] {
            &[]
        }
    }

    fn klass(fqn: &str) -> Class {
        Class {
            name: fqn.rsplit('.').next().unwrap_or(fqn).to_string(),
            fqn: fqn.to_string(),
            file: PathBuf::from(format!("{fqn}.java")),
            line_start: 1,
            line_end: 1,
            annotations: vec![Annotation {
                name: "Marker".into(),
                fqn: None,
                raw_args: None,
            }],
            ..Default::default()
        }
    }

    fn mk_module(id: &str, classes: Vec<Class>) -> Module {
        mk_module_at(id, PathBuf::from("/tmp/tour-suggest"), classes)
    }

    fn mk_module_at(id: &str, root: PathBuf, classes: Vec<Class>) -> Module {
        let mut m = Module {
            id: id.to_string(),
            root,
            ..Default::default()
        };
        for c in classes {
            m.classes.insert(c.fqn.clone(), c);
        }
        m
    }

    fn repo_with(modules: Vec<Module>) -> Repository {
        // Non-git path → commit_activity degrades to zero activity, no error.
        let mut r = Repository::new(PathBuf::from("/tmp/tour-suggest-nonexistent"));
        for m in modules {
            r.insert_module(m);
        }
        r
    }

    /// A throwaway on-disk git repo. `risk::compute` walks git history, so the
    /// top-class facts only populate on a real checkout — these tests build a
    /// tiny one, commit the class files, and clean up on drop.
    struct TempGitRepo {
        dir: PathBuf,
    }
    impl Drop for TempGitRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    impl TempGitRepo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "projectmind-tour-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            git2::Repository::init(&dir).unwrap();
            Self { dir }
        }

        /// Write `<fqn>.java` for every class of every module, then commit.
        fn commit_classes(&self, modules: &[Module]) {
            let git = git2::Repository::open(&self.dir).unwrap();
            let mut index = git.index().unwrap();
            for m in modules {
                for class in m.classes.values() {
                    let path = &class.file;
                    std::fs::write(
                        self.dir.join(path),
                        format!("class {} {{ void f(){{ if(x){{}} }} }}\n", class.name),
                    )
                    .unwrap();
                    index.add_path(path).unwrap();
                }
            }
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = git.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("t", "t@t").unwrap();
            git.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
    }

    #[test]
    fn empty_repo_yields_empty_ranking_and_overview_only() {
        let scaffold = suggest_tour(
            &repo_with(vec![]),
            &DummyFw { relations: vec![] },
            5,
            Persona::NewDev,
        );
        assert!(scaffold.ranking.is_empty());
        assert_eq!(scaffold.repo.modules_total, 0);
        assert_eq!(scaffold.repo.classes_total, 0);
        // Overview + closing step, no per-module steps.
        assert_eq!(scaffold.suggested_steps.len(), 2);
        assert_eq!(scaffold.suggested_steps[0].title, "Overview");
        assert_eq!(scaffold.suggested_steps[1].title, "Where change happens");
    }

    #[test]
    fn coupled_module_outranks_isolated_one() {
        // web → core (2 edges). core is depended on; web depends out.
        let ctrl = klass("g:web.Ctrl");
        let svc = klass("g:core.Svc");
        let lonely = klass("g:util.Lonely");
        let web = mk_module("g:web", vec![ctrl.clone()]);
        let core = mk_module("g:core", vec![svc.clone()]);
        let util = mk_module("g:util", vec![lonely]);
        let fw = DummyFw {
            relations: vec![
                (
                    "g:web".to_string(),
                    vec![
                        Relation {
                            from: ctrl.fqn.clone(),
                            to: svc.fqn.clone(),
                            kind: RelationKind::Injects,
                        },
                        Relation {
                            from: ctrl.fqn.clone(),
                            to: svc.fqn.clone(),
                            kind: RelationKind::Calls,
                        },
                    ],
                ),
                ("g:core".to_string(), vec![]),
                ("g:util".to_string(), vec![]),
            ],
        };
        let scaffold = suggest_tour(&repo_with(vec![web, core, util]), &fw, 5, Persona::NewDev);
        // util is coupling-free → ranks last.
        assert_eq!(scaffold.ranking.len(), 3);
        assert_ne!(scaffold.ranking.last().unwrap().module, "g:core");
        assert_eq!(scaffold.ranking.last().unwrap().module, "g:util");
        // The coupled pair scores above the isolated module.
        let util_score = scaffold
            .ranking
            .iter()
            .find(|m| m.module == "g:util")
            .unwrap()
            .score;
        let core_score = scaffold
            .ranking
            .iter()
            .find(|m| m.module == "g:core")
            .unwrap()
            .score;
        assert!(core_score > util_score);
    }

    #[test]
    fn ranking_is_deterministic() {
        let mods = || {
            let a = klass("g:a.A");
            let b = klass("g:b.B");
            vec![mk_module("g:a", vec![a]), mk_module("g:b", vec![b])]
        };
        let fw = DummyFw { relations: vec![] };
        let one = suggest_tour(&repo_with(mods()), &fw, 5, Persona::NewDev);
        let two = suggest_tour(&repo_with(mods()), &fw, 5, Persona::NewDev);
        let ids_one: Vec<_> = one.ranking.iter().map(|m| m.module.clone()).collect();
        let ids_two: Vec<_> = two.ranking.iter().map(|m| m.module.clone()).collect();
        assert_eq!(ids_one, ids_two);
    }

    #[test]
    fn top_cap_is_honoured() {
        let modules: Vec<Module> = (0..6)
            .map(|i| mk_module(&format!("g:m{i}"), vec![klass(&format!("g:m{i}.C"))]))
            .collect();
        let fw = DummyFw { relations: vec![] };
        let scaffold = suggest_tour(&repo_with(modules), &fw, 3, Persona::NewDev);
        assert_eq!(scaffold.ranking.len(), 3);
        // Overview + 3 module steps + closing = 5.
        assert_eq!(scaffold.suggested_steps.len(), 5);
    }

    #[test]
    fn top_zero_is_clamped_to_one() {
        let fw = DummyFw { relations: vec![] };
        let scaffold = suggest_tour(
            &repo_with(vec![mk_module("g:a", vec![klass("g:a.A")])]),
            &fw,
            0,
            Persona::NewDev,
        );
        assert_eq!(scaffold.ranking.len(), 1);
    }

    #[test]
    fn top_class_is_highest_fan_in() {
        // Two classes point at core.Hub → Hub has fan-in 2 and should be the
        // module's top class. Needs a real git repo for risk::compute.
        let git = TempGitRepo::new("fanin");
        let hub = klass("g:core.Hub");
        let leaf = klass("g:core.Leaf");
        let a = klass("g:web.A");
        let b = klass("g:web.B");
        let core = mk_module_at("g:core", git.dir.clone(), vec![hub.clone(), leaf]);
        let web = mk_module_at("g:web", git.dir.clone(), vec![a.clone(), b.clone()]);
        git.commit_classes(&[core.clone(), web.clone()]);

        let mut repo = Repository::new(git.dir.clone());
        repo.insert_module(core);
        repo.insert_module(web);
        let fw = DummyFw {
            relations: vec![(
                "g:web".to_string(),
                vec![
                    Relation {
                        from: a.fqn.clone(),
                        to: hub.fqn.clone(),
                        kind: RelationKind::Uses,
                    },
                    Relation {
                        from: b.fqn.clone(),
                        to: hub.fqn.clone(),
                        kind: RelationKind::Uses,
                    },
                ],
            )],
        };
        let scaffold = suggest_tour(&repo, &fw, 5, Persona::NewDev);
        let core_rank = scaffold
            .ranking
            .iter()
            .find(|m| m.module == "g:core")
            .unwrap();
        let top = core_rank.top_class.as_ref().expect("core has a top class");
        assert_eq!(top.fqn, "g:core.Hub");
        assert_eq!(top.fan_in, 2);
    }

    #[test]
    fn activity_signal_joins_chord_and_git_module_ids_maven() {
        // Regression for the dead activity signal: the chord side keys
        // modules by the engine id (`MavenModule::coordinate()`, i.e.
        // `groupId:artifactId`), while `commit_activity` runs its own
        // manifest discovery. Both sides must produce the same id or
        // `commits_90d` is always 0 and the 0.3 ranking weight is dead.
        let git = TempGitRepo::new("join-maven");
        std::fs::write(
            git.dir.join("pom.xml"),
            "<project><groupId>com.acme</groupId><artifactId>core</artifactId></project>",
        )
        .unwrap();
        let hub = klass("core.Hub");
        let core = mk_module_at("com.acme:core", git.dir.clone(), vec![hub]);
        git.commit_classes(std::slice::from_ref(&core));

        let mut repo = Repository::new(git.dir.clone());
        repo.insert_module(core);
        let scaffold = suggest_tour(&repo, &DummyFw { relations: vec![] }, 5, Persona::NewDev);

        let ranked = scaffold
            .ranking
            .iter()
            .find(|m| m.module == "com.acme:core")
            .expect("ranked module");
        assert_eq!(
            ranked.commits_90d, 1,
            "commit_activity ids must join onto the chord module id"
        );
        assert!(
            ranked
                .facts
                .iter()
                .any(|f| f.contains("1 commit(s) in 90d")),
            "facts must report the joined activity, got {:?}",
            ranked.facts
        );
    }

    #[test]
    fn activity_signal_joins_chord_and_git_module_ids_cargo() {
        // Same join for Cargo with a literal version: the engine id is
        // `name@version` (`CargoCrate::coordinate()`); a bare crate name on
        // the activity side never matched it.
        let git = TempGitRepo::new("join-cargo");
        std::fs::write(
            git.dir.join("Cargo.toml"),
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let engine = klass("core.Engine");
        let core = mk_module_at("core@0.1.0", git.dir.clone(), vec![engine]);
        git.commit_classes(std::slice::from_ref(&core));

        let mut repo = Repository::new(git.dir.clone());
        repo.insert_module(core);
        let scaffold = suggest_tour(&repo, &DummyFw { relations: vec![] }, 5, Persona::NewDev);

        let ranked = scaffold
            .ranking
            .iter()
            .find(|m| m.module == "core@0.1.0")
            .expect("ranked module");
        assert_eq!(
            ranked.commits_90d, 1,
            "cargo `name@version` ids must join onto the chord module id"
        );
    }

    #[test]
    fn persona_parse_falls_back_to_new_dev() {
        assert_eq!(Persona::parse("architect"), Persona::Architect);
        assert_eq!(Persona::parse("new-dev"), Persona::NewDev);
        assert_eq!(Persona::parse("garbage"), Persona::NewDev);
    }

    #[test]
    fn steps_point_class_targets_at_top_classes() {
        let git = TempGitRepo::new("steps");
        let hub = klass("g:core.Hub");
        let a = klass("g:web.A");
        let core = mk_module_at("g:core", git.dir.clone(), vec![hub.clone()]);
        let web = mk_module_at("g:web", git.dir.clone(), vec![a.clone()]);
        git.commit_classes(&[core.clone(), web.clone()]);

        let mut repo = Repository::new(git.dir.clone());
        repo.insert_module(core);
        repo.insert_module(web);
        let fw = DummyFw {
            relations: vec![(
                "g:web".to_string(),
                vec![Relation {
                    from: a.fqn.clone(),
                    to: hub.fqn.clone(),
                    kind: RelationKind::Uses,
                }],
            )],
        };
        let scaffold = suggest_tour(&repo, &fw, 5, Persona::Architect);
        let core_step = scaffold
            .suggested_steps
            .iter()
            .find(|s| s.title == "g:core")
            .expect("core module step");
        match &core_step.target {
            StepTarget::Class { fqn } => assert_eq!(fqn, "g:core.Hub"),
            other => panic!("expected class target, got {other:?}"),
        }
        // Closing atlas step rings the hotspot.
        let closing = scaffold.suggested_steps.last().unwrap();
        match &closing.target {
            StepTarget::Atlas { highlight } => {
                assert!(highlight.contains(&"g:core.Hub".to_string()));
            }
            other => panic!("expected atlas target, got {other:?}"),
        }
    }
}
