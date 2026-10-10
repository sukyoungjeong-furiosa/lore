use lore_local::{LocalStoreConnection, LocalStoreRegistry};
use std::sync::{Arc, Barrier};

#[test]
fn concurrent_initialization_reuses_one_personal_store_and_preserves_all_registrations() {
    let temp = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|index| {
            let home = temp.path().to_owned();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let connection = LocalStoreRegistry::new(&home).ensure_personal().unwrap();
                connection
                    .register(
                        &[format!("repository-{index}")],
                        &[format!("backend-{index}")],
                    )
                    .unwrap();
                connection.store
            })
        })
        .collect();
    let ids: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(ids.iter().all(|id| *id == ids[0]));
    let registry = LocalStoreRegistry::new(temp.path());
    assert_eq!(registry.list().unwrap().len(), 1);
    let descriptor = registry.get("personal").unwrap().descriptor().unwrap();
    assert_eq!(descriptor.repositories.len(), 8);
    assert_eq!(descriptor.backends.len(), 8);
}

#[test]
fn a_replaced_store_is_rejected_and_missing_personal_storage_is_not_recreated_silently() {
    let temp = tempfile::tempdir().unwrap();
    let registry = LocalStoreRegistry::new(temp.path());
    let personal = registry.ensure_personal().unwrap();
    let other = registry.create("other", None).unwrap();
    std::fs::copy(
        other.directory.join("store.json"),
        personal.directory.join("store.json"),
    )
    .unwrap();
    assert_eq!(
        personal.descriptor().unwrap_err().kind,
        lore_core::ErrorKind::Conflict
    );
    assert_eq!(
        registry.ensure_personal().unwrap_err().kind,
        lore_core::ErrorKind::Conflict
    );
    assert_eq!(
        LocalStoreConnection::open(&personal.directory)
            .unwrap()
            .store,
        other.store
    );
    std::fs::remove_dir_all(&personal.directory).unwrap();
    assert_eq!(
        registry.ensure_personal().unwrap_err().kind,
        lore_core::ErrorKind::NotFound
    );
    assert!(!personal.directory.exists());
}
