// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixture-based tests for the Pattern Lens detectors (#159).
//!
//! Each detector gets a *clean* repo (rule holds) and a *drifting* repo (rule
//! violated), written to a temp dir as real `.java` source, parsed through the
//! full engine (Java language plugin + Spring framework plugin), then checked.
//! This exercises the whole pipeline — including the source-body scan that the
//! `Repository` and `DiOnly` detectors rely on, which the unit tests in
//! `crates/core/src/patterns.rs` can only cover synthetically.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use projectmind_core::patterns::{self, Pattern, PatternConfig, Scope, CONFIDENCE_FLOOR};
use projectmind_core::{Engine, Repository};
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;

fn uniq() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    ts ^ (N.fetch_add(1, Ordering::Relaxed) << 32)
}

/// A throwaway repo populated with named `.java` files, auto-removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "projectmind-patterns-{}-{}",
            std::process::id(),
            uniq()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for (name, body) in files {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, body).unwrap();
        }
        Self { root }
    }

    fn open(&self) -> Repository {
        let mut engine = Engine::new();
        engine.register_language(Box::new(JavaPlugin::new()));
        engine.register_framework(Box::new(SpringPlugin::new()));
        engine.open_repo(&self.root).expect("open repo")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn check(repo: &Repository, pattern: Pattern) -> patterns::PatternResult {
    patterns::check_with_config(repo, pattern, &Scope::default(), &PatternConfig::default())
}

// ---------------------------------------------------------------------------
// Repository: only @Repository may touch EntityManager / JdbcTemplate directly
// ---------------------------------------------------------------------------

#[test]
fn repository_clean_repo_holds() {
    let fx = Fixture::new(&[
        (
            "UserService.java",
            "package demo.service;\n\
             @Service\n\
             public class UserService {\n\
             \x20   private final UserRepository repo;\n\
             \x20   public UserService(UserRepository repo) { this.repo = repo; }\n\
             \x20   public User find(long id) { return repo.findById(id); }\n\
             }\n",
        ),
        (
            "UserRepository.java",
            "package demo.repository;\n\
             @Repository\n\
             public class UserRepository {\n\
             \x20   private EntityManager entityManager;\n\
             \x20   public User findById(long id) { return entityManager.find(User.class, id); }\n\
             }\n",
        ),
    ]);
    let repo = fx.open();
    let res = check(&repo, Pattern::Repository);
    assert!(
        res.violations.is_empty(),
        "clean repo should hold, got: {:?}",
        res.violations
    );
}

#[test]
fn repository_service_using_entitymanager_field_drifts() {
    let fx = Fixture::new(&[(
        "OrderService.java",
        "package demo.service;\n\
         @Service\n\
         public class OrderService {\n\
         \x20   @PersistenceContext\n\
         \x20   private EntityManager entityManager;\n\
         \x20   public void save(Order o) { entityManager.persist(o); }\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::Repository);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    let v = &res.violations[0];
    assert!(v.confidence >= CONFIDENCE_FLOOR);
    assert!(v.message.contains("persistence"));
}

#[test]
fn repository_service_calling_entitymanager_in_body_drifts() {
    // No EntityManager *field* — only a method-body call. This can only be
    // caught by the source-body scan.
    let fx = Fixture::new(&[(
        "ReportService.java",
        "package demo.service;\n\
         @Service\n\
         public class ReportService {\n\
         \x20   private final Helper helper;\n\
         \x20   public ReportService(Helper h) { this.helper = h; }\n\
         \x20   public long count() {\n\
         \x20       return getEntityManager().createQuery(\"...\").getSingleResult();\n\
         \x20   }\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::Repository);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    assert!(res.violations[0].message.contains("EntityManager"));
}

// ---------------------------------------------------------------------------
// DI-only: no `new XxxService()` inside a @Component
// ---------------------------------------------------------------------------

#[test]
fn di_clean_repo_holds() {
    let fx = Fixture::new(&[
        (
            "UserController.java",
            "package demo.web;\n\
             @RestController\n\
             public class UserController {\n\
             \x20   private final UserService service;\n\
             \x20   public UserController(UserService service) { this.service = service; }\n\
             }\n",
        ),
        (
            "UserService.java",
            "package demo.service;\n@Service\npublic class UserService {}\n",
        ),
    ]);
    let repo = fx.open();
    let res = check(&repo, Pattern::DiOnly);
    assert!(
        res.violations.is_empty(),
        "clean repo should hold, got: {:?}",
        res.violations
    );
}

#[test]
fn di_manual_new_of_bean_drifts() {
    let fx = Fixture::new(&[
        (
            "UserController.java",
            "package demo.web;\n\
             @RestController\n\
             public class UserController {\n\
             \x20   private final UserService service = new UserService();\n\
             }\n",
        ),
        (
            "UserService.java",
            "package demo.service;\n@Service\npublic class UserService {}\n",
        ),
    ]);
    let repo = fx.open();
    let res = check(&repo, Pattern::DiOnly);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    let v = &res.violations[0];
    assert!(v.confidence >= CONFIDENCE_FLOOR);
    assert!(v.message.contains("UserService"));
}

// ---------------------------------------------------------------------------
// Layered: web classes must not reference the persistence layer directly
// ---------------------------------------------------------------------------

#[test]
fn layered_clean_web_holds() {
    let fx = Fixture::new(&[(
        "HomeController.java",
        "package demo.web;\n\
         @Controller\n\
         public class HomeController {\n\
         \x20   private final demo.service.HomeService service;\n\
         \x20   public HomeController(demo.service.HomeService s) { this.service = s; }\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::Layered);
    assert!(res.violations.is_empty(), "got: {:?}", res.violations);
}

#[test]
fn layered_web_touching_repository_drifts() {
    let fx = Fixture::new(&[(
        "HomeController.java",
        "package demo.web;\n\
         @Controller\n\
         public class HomeController {\n\
         \x20   private demo.repository.UserRepository repo;\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::Layered);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    assert!(res.violations[0].message.contains("persistence"));
}

// ---------------------------------------------------------------------------
// @Transactional boundary
// ---------------------------------------------------------------------------

#[test]
fn tx_on_service_clean_holds() {
    let fx = Fixture::new(&[(
        "BillingService.java",
        "package demo.service;\n\
         @Service\n\
         public class BillingService {\n\
         \x20   @Transactional\n\
         \x20   public void charge() {}\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::TxOnService);
    assert!(res.violations.is_empty(), "got: {:?}", res.violations);
    assert_eq!(res.holds.iter().map(|h| h.count).sum::<u32>(), 1);
}

#[test]
fn tx_on_controller_drifts() {
    let fx = Fixture::new(&[(
        "BillingController.java",
        "package demo.web;\n\
         @RestController\n\
         public class BillingController {\n\
         \x20   @Transactional\n\
         \x20   public void charge() {}\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::TxOnService);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    assert!(res.violations[0].message.contains("@Transactional"));
}

// ---------------------------------------------------------------------------
// No-static-state
// ---------------------------------------------------------------------------

#[test]
fn no_static_state_clean_holds() {
    let fx = Fixture::new(&[(
        "ConfigHolder.java",
        "package demo;\n\
         @Component\n\
         public class ConfigHolder {\n\
         \x20   private static final String NAME = \"x\";\n\
         \x20   private String value;\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::NoStaticState);
    assert!(res.violations.is_empty(), "got: {:?}", res.violations);
}

#[test]
fn no_static_state_mutable_cache_drifts() {
    let fx = Fixture::new(&[(
        "CacheComponent.java",
        "package demo;\n\
         @Component\n\
         public class CacheComponent {\n\
         \x20   private static Map<String, String> CACHE = new HashMap<>();\n\
         }\n",
    )]);
    let repo = fx.open();
    let res = check(&repo, Pattern::NoStaticState);
    assert_eq!(res.violations.len(), 1, "got: {:?}", res.violations);
    assert!(res.violations[0].confidence >= CONFIDENCE_FLOOR);
}

// ---------------------------------------------------------------------------
// check_all + config
// ---------------------------------------------------------------------------

#[test]
fn check_all_runs_every_enabled_detector() {
    let fx = Fixture::new(&[(
        "Svc.java",
        "package demo.service;\n@Service\npublic class Svc {}\n",
    )]);
    let repo = fx.open();
    let results = patterns::check_all(&repo, &Scope::default(), &PatternConfig::default());
    assert_eq!(results.len(), 5, "all five detectors run by default");
}

#[test]
fn config_disable_removes_detector_from_check_all() {
    let fx = Fixture::new(&[(
        "Svc.java",
        "package demo.service;\n@Service\npublic class Svc {}\n",
    )]);
    let repo = fx.open();
    let cfg = PatternConfig::parse("[patterns]\ndisable = [\"NoStaticState\"]\n").unwrap();
    let results = patterns::check_all(&repo, &Scope::default(), &cfg);
    assert_eq!(results.len(), 4);
    assert!(!results.iter().any(|r| r.pattern == Pattern::NoStaticState));
}
