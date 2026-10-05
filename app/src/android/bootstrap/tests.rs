use std::{
    fs::{self, File},
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cinnabar-bootstrap-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn resources(&self) -> PathBuf {
        self.0.join("resources")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn concurrent_bootstrap_cannot_create_the_same_payload_file() {
    let fixture = Fixture::new();
    let coordinator = Arc::new(Coordinator::new());
    let first = coordinator.begin(1, || {});
    let staged = first.fresh_staging(&fixture.resources()).unwrap();
    fs::create_dir_all(&staged).unwrap();
    let (admitted, admission) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let resources = fixture.resources();
    let second_coordinator = coordinator.clone();
    let second = std::thread::spawn(move || -> io::Result<()> {
        let attempt = second_coordinator.begin(2, || {
            admitted.send(false).unwrap();
        });
        let staged = attempt.fresh_staging(&resources)?;
        fs::create_dir_all(&staged)?;
        admitted.send(true).unwrap();
        released.recv().unwrap();
        File::create_new(staged.join("payload"))?;
        Ok(())
    });
    let overlapping = admission.recv().unwrap();
    fs::write(staged.join("payload"), b"first worker").unwrap();
    assert_eq!(fs::read(staged.join("payload")).unwrap(), b"first worker");
    if !overlapping {
        assert_eq!(coordinator.owner(), Some(1));
        drop(first);
        assert!(admission.recv().unwrap());
        assert_eq!(coordinator.owner(), Some(2));
    }
    release.send(()).unwrap();
    let result = second.join().unwrap();
    assert!(
        !overlapping,
        "a second bootstrap entered the owned staging directory: {result:?}"
    );
    result.unwrap();
    assert_eq!(fs::read(staged.join("payload")).unwrap(), b"");
    assert!(coordinator.owner().is_none());
}

#[test]
fn retry_after_interruption_replaces_the_partial_stage() {
    let fixture = Fixture::new();
    let coordinator = Coordinator::new();
    {
        let first = coordinator.begin(1, || {});
        let staged = first.fresh_staging(&fixture.resources()).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("payload"), b"interrupted").unwrap();
        assert!(coordinator.cancel(|owner| *owner == 1));
        assert!(coordinator.cancellation().load(Ordering::Relaxed));
    }
    let retry = coordinator.begin(2, || {});
    assert!(!coordinator.cancellation().load(Ordering::Relaxed));
    let staged = retry.fresh_staging(&fixture.resources()).unwrap();
    fs::create_dir_all(&staged).unwrap();
    File::create_new(staged.join("payload")).unwrap();
}

#[test]
fn destroyed_previous_activity_cannot_cancel_its_successor() {
    let coordinator = Coordinator::new();
    drop(coordinator.begin(1, || {}));
    let _next = coordinator.begin(2, || {});
    assert!(!coordinator.cancel(|owner| *owner == 1));
    assert!(!coordinator.cancellation().load(Ordering::Relaxed));
    assert!(coordinator.cancel(|owner| *owner == 2));
    assert!(coordinator.cancellation().load(Ordering::Relaxed));
}

#[test]
fn completed_activity_cannot_cancel_without_an_active_attempt() {
    let coordinator = Coordinator::new();
    drop(coordinator.begin(1, || {}));
    assert!(coordinator.owner().is_none());
    assert!(!coordinator.cancel(|owner| *owner == 1));
}
