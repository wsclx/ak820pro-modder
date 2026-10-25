use super::*;
use std::collections::BTreeMap;

fn mapping(source: u16, destination: u16) -> NativeMapping {
    NativeMapping {
        source: PAGE | u64::from(source),
        destination: PAGE | u64::from(destination),
    }
}
fn draft() -> Vec<Remap> {
    vec![Remap {
        source: 4,
        destination: 5,
    }]
}
fn initial() -> Snapshot {
    Snapshot {
        attachment: Attachment {
            boot_session: "boot-a".into(),
            usb_registry_id: 42,
        },
        services: vec![
            Service {
                registry_id: 10,
                map: vec![mapping(80, 81)],
            },
            Service {
                registry_id: 20,
                map: vec![mapping(90, 91)],
            },
        ],
    }
}
#[derive(Clone, Copy)]
enum Fault {
    Reject,
    AfterWrite,
    Noop,
    Corrupt,
    Disconnect,
}
struct MockBackend {
    current: Snapshot,
    writes: Vec<(u64, Vec<NativeMapping>)>,
    faults: BTreeMap<usize, Fault>,
    fail_read: Option<u64>,
}
impl MockBackend {
    fn new() -> Self {
        Self {
            current: initial(),
            writes: vec![],
            faults: BTreeMap::new(),
            fail_read: None,
        }
    }
    fn service(&mut self, id: u64) -> Result<&mut Service, String> {
        self.current
            .services
            .iter_mut()
            .find(|s| s.registry_id == id)
            .ok_or_else(|| "disconnected".into())
    }
}
impl Backend for MockBackend {
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        Ok(self.current.clone())
    }
    fn read(&mut self, id: u64) -> Result<Vec<NativeMapping>, String> {
        if self.fail_read == Some(id) {
            return Err("read failed".into());
        }
        Ok(self.service(id)?.map.clone())
    }
    fn write(&mut self, id: u64, map: &[NativeMapping]) -> Result<(), String> {
        self.writes.push((id, map.to_vec()));
        match self.faults.get(&self.writes.len()).copied() {
            Some(Fault::Reject) => Err("write rejected".into()),
            Some(Fault::AfterWrite) => {
                self.service(id)?.map = map.to_vec();
                Err("write reported failure after taking effect".into())
            }
            Some(Fault::Noop) => Ok(()),
            Some(Fault::Corrupt) => {
                self.service(id)?.map = vec![mapping(25, 26)];
                Ok(())
            }
            Some(Fault::Disconnect) => {
                self.current.services.retain(|s| s.registry_id != id);
                Err("disconnected".into())
            }
            None => {
                self.service(id)?.map = map.to_vec();
                Ok(())
            }
        }
    }
}
#[derive(Default)]
struct MemoryStore {
    record: Option<Record>,
    saves: usize,
    fail_saves: BTreeSet<usize>,
}
impl Store for MemoryStore {
    fn load(&self) -> Result<Option<Record>, String> {
        Ok(self.record.clone())
    }
    fn save(&mut self, record: Option<Record>) -> Result<(), String> {
        self.saves += 1;
        if self.fail_saves.contains(&self.saves) {
            return Err("backup store failed".into());
        }
        self.record = record;
        Ok(())
    }
}

#[test]
fn two_services_keep_separate_baselines_and_external_changes_on_undo() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    assert_eq!(
        change(&mut backend, &mut store, draft(), false).unwrap(),
        draft()
    );
    let Some(Record::Applied(backup)) = &store.record else {
        panic!("missing backup")
    };
    assert_ne!(backup.services[0].baseline, backup.services[1].baseline);
    // External unrelated deletion/addition after Apply must survive Undo.
    backend
        .service(10)
        .unwrap()
        .map
        .retain(|m| m.source != PAGE | 80);
    backend.service(20).unwrap().map.push(mapping(100, 101));
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert!(backend.service(10).unwrap().map.is_empty());
    assert_eq!(
        backend.service(20).unwrap().map,
        vec![mapping(90, 91), mapping(100, 101)]
    );
    assert!(store.record.is_none());
}
#[test]
fn applying_a_new_draft_removes_only_previous_owned_entries() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    let next = vec![Remap {
        source: 6,
        destination: 7,
    }];
    change(&mut backend, &mut store, next.clone(), false).unwrap();
    assert_eq!(
        backend.service(10).unwrap().map,
        vec![mapping(6, 7), mapping(80, 81)]
    );
    assert_eq!(requested(store.record.as_ref().unwrap()).unwrap(), next);
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current, initial());
}
#[test]
fn existing_identical_mapping_is_not_owned_or_removed() {
    let mut backend = MockBackend::new();
    backend.service(10).unwrap().map.insert(0, mapping(4, 5));
    let original = backend.current.clone();
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current, original);
}
#[test]
fn preflight_conflict_on_second_service_prevents_every_write() {
    let mut backend = MockBackend::new();
    backend.service(20).unwrap().map.push(mapping(4, 9));
    let mut store = MemoryStore::default();
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    assert!(backend.writes.is_empty());
    assert_eq!(store.saves, 0);
}
#[test]
fn denied_noop_and_effective_failed_writes_restore_both_services() {
    for fault in [Fault::Reject, Fault::AfterWrite, Fault::Noop] {
        let mut backend = MockBackend::new();
        backend.faults.insert(2, fault);
        let mut store = MemoryStore::default();
        let error = change(&mut backend, &mut store, draft(), false).unwrap_err();
        assert!(error.contains("previous maps restored"), "{error}");
        assert_eq!(backend.current, initial());
        assert!(store.record.is_none());
    }
}
#[test]
fn rollback_failure_retains_a_journal_and_can_be_recovered_explicitly() {
    let mut backend = MockBackend::new();
    backend.faults.insert(2, Fault::AfterWrite);
    backend.faults.insert(3, Fault::Reject); // rollback service two fails
    let mut store = MemoryStore::default();
    let error = change(&mut backend, &mut store, draft(), false).unwrap_err();
    assert!(error.contains("Recovery incomplete"), "{error}");
    assert_eq!(backend.service(10).unwrap().map, initial().services[0].map);
    assert!(matches!(store.record, Some(Record::Pending(_))));
    let writes = backend.writes.len();
    assert!(change(&mut backend, &mut store, draft(), false)
        .unwrap_err()
        .contains("recovery"));
    assert_eq!(backend.writes.len(), writes);
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current, initial());
    assert!(store.record.is_none());
}
#[test]
fn unexpected_readback_is_preserved_and_other_services_are_rolled_back() {
    let mut backend = MockBackend::new();
    backend.faults.insert(2, Fault::Corrupt);
    let mut store = MemoryStore::default();
    let error = change(&mut backend, &mut store, draft(), false).unwrap_err();
    assert!(error.contains("manual review"), "{error}");
    assert_eq!(backend.service(10).unwrap().map, initial().services[0].map);
    assert_eq!(backend.service(20).unwrap().map, vec![mapping(25, 26)]);
    assert!(matches!(store.record, Some(Record::Pending(_))));
}
#[test]
fn disconnected_service_keeps_recovery_record() {
    let mut backend = MockBackend::new();
    backend.faults.insert(2, Fault::Disconnect);
    let mut store = MemoryStore::default();
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    assert_eq!(backend.service(10).unwrap().map, initial().services[0].map);
    assert!(matches!(store.record, Some(Record::Pending(_))));
}
#[test]
fn journal_persistence_failure_prevents_mutations() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore {
        fail_saves: BTreeSet::from([1]),
        ..Default::default()
    };
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    assert!(backend.writes.is_empty());
    assert_eq!(backend.current, initial());
}
#[test]
fn final_record_failure_rolls_back_and_failed_cleanup_remains_recoverable() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore {
        fail_saves: BTreeSet::from([2, 3]),
        ..Default::default()
    };
    let error = change(&mut backend, &mut store, draft(), false).unwrap_err();
    assert!(error.contains("Maps restored"), "{error}");
    assert_eq!(backend.current, initial());
    assert!(matches!(store.record, Some(Record::Pending(_))));
    let writes = backend.writes.len();
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.writes.len(), writes); // finalize record only
    assert!(store.record.is_none());
}
#[test]
fn undo_deletion_failure_restores_applied_state_and_keeps_ownership() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    let applied = backend.current.clone();
    store.fail_saves.insert(store.saves + 2);
    assert!(change(&mut backend, &mut store, vec![], true).is_err());
    assert_eq!(backend.current, applied);
    assert!(matches!(store.record, Some(Record::Applied(_))));
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current, initial());
}
#[test]
fn external_edit_of_owned_source_blocks_undo_without_overwriting_it() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    backend.service(20).unwrap().map[0] = mapping(4, 99);
    let current = backend.current.clone();
    let writes = backend.writes.len();
    assert!(change(&mut backend, &mut store, vec![], true)
        .unwrap_err()
        .contains("Another tool"));
    assert_eq!(backend.writes.len(), writes);
    assert_eq!(backend.current, current);
}
#[test]
fn service_order_does_not_assign_baselines_but_new_attachment_is_refused() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    backend.current.services.reverse();
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(snapshot(&mut backend).unwrap(), initial());
    for kind in 0..3 {
        let mut backend = MockBackend::new();
        let mut store = MemoryStore::default();
        change(&mut backend, &mut store, draft(), false).unwrap();
        match kind {
            0 => backend.current.attachment.boot_session = "boot-b".into(),
            1 => backend.current.attachment.usb_registry_id = 43,
            _ => backend.current.services[0].registry_id = 11,
        }
        let writes = backend.writes.len();
        assert!(change(&mut backend, &mut store, draft(), false)
            .unwrap_err()
            .contains("attachment or services changed"));
        assert_eq!(backend.writes.len(), writes);
    }
}
#[test]
fn legacy_backup_migrates_only_a_single_service_matching_applied_state() {
    let old = Record::Legacy {
        baseline: vec![mapping(80, 81)],
        applied: vec![mapping(4, 5)],
    };
    let mut backend = MockBackend::new();
    let mut store = MemoryStore {
        record: Some(old.clone()),
        ..Default::default()
    };
    assert!(change(&mut backend, &mut store, draft(), false)
        .unwrap_err()
        .contains("Migration"));
    assert!(backend.writes.is_empty());
    backend.current.services.truncate(1);
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    backend.current.services[0].map = vec![mapping(4, 5)];
    change(&mut backend, &mut store, draft(), false).unwrap();
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current.services[0].map, vec![mapping(80, 81)]);
}
#[test]
fn invalid_draft_and_duplicate_service_ids_are_refused_before_writes() {
    let mut backend = MockBackend::new();
    let mut store = MemoryStore::default();
    assert!(change(&mut backend, &mut store, vec![draft()[0].clone(); 2], false).is_err());
    backend.current.services[1].registry_id = 10;
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    assert!(backend.writes.is_empty());
    assert_eq!(store.saves, 0);
    assert!(editor_map(&[NativeMapping {
        source: PAGE | 0x10004,
        destination: PAGE | 5
    }])
    .is_err());
}
#[test]
fn full_width_non_keyboard_entries_survive_apply_and_undo() {
    let mut backend = MockBackend::new();
    let external = NativeMapping {
        source: (12 << 32) | 0x12345,
        destination: (12 << 32) | 0x23456,
    };
    backend.current.services[0].map.push(external.clone());
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap();
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert!(backend.current.services[0].map.contains(&external));
}
#[test]
fn recovery_after_restart_uses_the_serialized_journal() {
    let mut backend = MockBackend::new();
    backend.faults.insert(2, Fault::AfterWrite);
    backend.faults.insert(3, Fault::Reject);
    let mut store = MemoryStore::default();
    change(&mut backend, &mut store, draft(), false).unwrap_err();
    let bytes = serde_json::to_vec(&store.record).unwrap();
    let mut reopened = MemoryStore {
        record: serde_json::from_slice(&bytes).unwrap(),
        ..Default::default()
    };
    change(&mut backend, &mut reopened, vec![], true).unwrap();
    assert_eq!(backend.current, initial());
    assert!(reopened.record.is_none());
}
#[test]
fn unreadable_service_leaves_a_recovery_record_without_guessing() {
    let mut backend = MockBackend::new();
    backend.fail_read = Some(20);
    let mut store = MemoryStore::default();
    assert!(change(&mut backend, &mut store, draft(), false).is_err());
    assert_eq!(backend.service(10).unwrap().map, initial().services[0].map);
    assert!(matches!(store.record, Some(Record::Pending(_))));
}

#[test]
fn file_store_reads_legacy_round_trips_journals_and_rejects_unknown_versions() {
    let path = std::env::temp_dir().join(format!("ak820-remap-store-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let mut store = FileStore(&path);
    assert!(store.load().unwrap().is_none());
    std::fs::write(path.join(FILE), br#"{"baseline":[],"applied":[{"HIDKeyboardModifierMappingSrc":"30064771076","HIDKeyboardModifierMappingDst":"30064771077"}]}"#).unwrap();
    assert_eq!(requested(&store.load().unwrap().unwrap()).unwrap(), draft());
    let mut backend = MockBackend::new();
    let mut memory = MemoryStore::default();
    backend.faults.insert(2, Fault::AfterWrite);
    backend.faults.insert(3, Fault::Reject);
    change(&mut backend, &mut memory, draft(), false).unwrap_err();
    store.save(memory.record).unwrap();
    let record = store.load().unwrap().unwrap();
    assert!(matches!(record, Record::Pending(_)));
    assert_eq!(requested(&record).unwrap(), draft());
    change(&mut backend, &mut store, vec![], true).unwrap();
    assert_eq!(backend.current, initial());
    assert!(store.load().unwrap().is_none());
    std::fs::write(
        path.join(FILE),
        br#"{"version":99,"record":{"state":"legacy","data":{"baseline":[],"applied":[]}}}"#,
    )
    .unwrap();
    assert!(store.load().unwrap_err().contains("version"));
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "temporarily adds F24 identity on the connected USB AK820 services, verifies and restores each baseline"]
fn usb_remap_round_trip_and_undo() {
    let path = std::env::temp_dir().join(format!("ak820-remap-hardware-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    eprintln!("Recovery directory: {}", path.display());
    let mut backend = native::Native::open().unwrap();
    let before = snapshot(&mut backend).unwrap();
    assert!(
        before
            .services
            .iter()
            .all(|s| s.map.iter().all(|m| m.source != PAGE | 115)),
        "F24 already mapped; no changes made"
    );
    let mut store = FileStore(&path);
    let remaps = vec![Remap {
        source: 115,
        destination: 115,
    }];
    let applied = change(&mut backend, &mut store, remaps, false);
    let observed = snapshot(&mut backend);
    // Always attempt cleanup before asserting a successful Apply/read-back.
    let undo = if store.load().unwrap().is_some() {
        change(&mut backend, &mut store, vec![], true)
    } else {
        Ok(vec![])
    };
    let after = snapshot(&mut backend);
    assert!(
        undo.is_ok(),
        "Undo failed; recovery retained in {}: {undo:?}",
        path.display()
    );
    assert_eq!(
        after.unwrap(),
        before,
        "Baseline mismatch; recovery retained in {}",
        path.display()
    );
    assert!(store.load().unwrap().is_none());
    std::fs::remove_dir_all(&path).unwrap();
    applied.unwrap();
    let observed = observed.unwrap();
    assert!(observed
        .services
        .iter()
        .all(|s| s.map.contains(&mapping(115, 115))));
    eprintln!(
        "Verified F24 identity and restored all {} services",
        before.services.len()
    );
}
