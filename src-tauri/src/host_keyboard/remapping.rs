//! Per-attachment remaps and a write-ahead recovery record. No firmware I/O.
use super::{NativeMapping, Remap, PAGE};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path, sync::Mutex};

#[cfg(target_os = "macos")]
#[path = "remapping_native.rs"]
mod native;
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub struct Native;
    impl Native {
        pub fn open() -> Result<Self, String> {
            Err("Mac remapping requires macOS".into())
        }
    }
    impl Backend for Native {
        fn snapshot(&mut self) -> Result<Snapshot, String> {
            Err("Mac remapping requires macOS".into())
        }
        fn read(&mut self, _: u64) -> Result<Vec<NativeMapping>, String> {
            Err("Mac remapping requires macOS".into())
        }
        fn write(&mut self, _: u64, _: &[NativeMapping]) -> Result<(), String> {
            Err("Mac remapping requires macOS".into())
        }
    }
}

const FILE: &str = "host-remap-backup.json";
static LOCK: Mutex<()> = Mutex::new(());

// Registry IDs identify an attachment, not a persistent device. The boot UUID
// also prevents an old backup from matching IDs reused after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Attachment {
    boot_session: String,
    usb_registry_id: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Service {
    registry_id: u64,
    map: Vec<NativeMapping>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    attachment: Attachment,
    services: Vec<Service>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceBackup {
    registry_id: u64,
    baseline: Vec<NativeMapping>,
    applied: Vec<NativeMapping>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Backup {
    attachment: Attachment,
    requested: Vec<Remap>,
    services: Vec<ServiceBackup>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    before: Snapshot,
    after: Snapshot,
    previous: Option<Backup>,
    next: Option<Backup>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case")]
enum Record {
    Applied(Backup),
    Pending(Pending),
    // Only read from the unversioned upstream file. Never inferred per service.
    Legacy {
        baseline: Vec<NativeMapping>,
        applied: Vec<NativeMapping>,
    },
}
#[derive(Serialize, Deserialize)]
struct Versioned {
    version: u8,
    record: Record,
}
trait Backend {
    fn snapshot(&mut self) -> Result<Snapshot, String>;
    fn read(&mut self, service: u64) -> Result<Vec<NativeMapping>, String>;
    fn write(&mut self, service: u64, map: &[NativeMapping]) -> Result<(), String>;
}
trait Store {
    fn load(&self) -> Result<Option<Record>, String>;
    // A failed save must leave the previous complete record in place.
    fn save(&mut self, record: Option<Record>) -> Result<(), String>;
}
struct FileStore<'a>(&'a Path);
impl Store for FileStore<'_> {
    fn load(&self) -> Result<Option<Record>, String> {
        let bytes = match std::fs::read(self.0.join(FILE)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let record = if value.get("version").is_some() {
            let versioned: Versioned = serde_json::from_value(value).map_err(|e| e.to_string())?;
            if versioned.version != 2 || matches!(versioned.record, Record::Legacy { .. }) {
                return Err(
                    "Unsupported Mac mapping backup version; leave the file unchanged".into(),
                );
            }
            versioned.record
        } else {
            #[derive(Deserialize)]
            struct Legacy {
                baseline: Vec<NativeMapping>,
                applied: Vec<NativeMapping>,
            }
            let old: Legacy = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Record::Legacy {
                baseline: old.baseline,
                applied: old.applied,
            }
        };
        Ok(Some(record))
    }
    fn save(&mut self, record: Option<Record>) -> Result<(), String> {
        let path = self.0.join(FILE);
        if let Some(record) = record {
            use std::io::Write;
            std::fs::create_dir_all(self.0).map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&Versioned { version: 2, record })
                .map_err(|e| e.to_string())?;
            let temp = self.0.join(format!("{FILE}.tmp"));
            let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            std::fs::rename(temp, path).map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

fn canonical(map: &[NativeMapping]) -> Result<Vec<NativeMapping>, String> {
    let mut map = map.to_vec();
    map.sort();
    if map
        .iter()
        .any(|m| m.source > i64::MAX as u64 || m.destination > i64::MAX as u64)
        || map.windows(2).any(|pair| pair[0].source == pair[1].source)
    {
        return Err("Invalid or duplicate native mapping; no changes made".into());
    }
    Ok(map)
}
fn native_map(remaps: &[Remap]) -> Result<Vec<NativeMapping>, String> {
    if remaps.len() > 128
        || remaps
            .iter()
            .any(|r| !(4..=231).contains(&r.source) || !(4..=231).contains(&r.destination))
    {
        return Err("Use unique keyboard usages from 4 to 231".into());
    }
    canonical(
        &remaps
            .iter()
            .map(|r| NativeMapping {
                source: PAGE | u64::from(r.source),
                destination: PAGE | u64::from(r.destination),
            })
            .collect::<Vec<_>>(),
    )
}
fn editor_map(map: &[NativeMapping]) -> Result<Vec<Remap>, String> {
    let remaps = map.iter().map(|m| {
        if m.source & !0xffff != PAGE || m.destination & !0xffff != PAGE {
            return Err("Existing mappings use a different usage page; start a new draft to preserve them".into());
        }
        Ok(Remap { source: (m.source & 0xffff) as u16, destination: (m.destination & 0xffff) as u16 })
    }).collect::<Result<Vec<_>, String>>()?;
    native_map(&remaps)?;
    Ok(remaps)
}
fn normalize_snapshot(snapshot: &mut Snapshot) -> Result<(), String> {
    if snapshot.attachment.boot_session.trim().is_empty()
        || snapshot.attachment.usb_registry_id == 0
        || snapshot.services.is_empty()
    {
        return Err("Cannot identify the physical USB keyboard attachment".into());
    }
    snapshot.services.sort_by_key(|s| s.registry_id);
    if snapshot.services.iter().any(|s| s.registry_id == 0)
        || snapshot
            .services
            .windows(2)
            .any(|pair| pair[0].registry_id == pair[1].registry_id)
    {
        return Err("Invalid keyboard service identity".into());
    }
    for service in &mut snapshot.services {
        service.map = canonical(&service.map)?;
    }
    Ok(())
}
fn same_attachment(a: &Snapshot, b: &Snapshot) -> Result<(), String> {
    if a.attachment != b.attachment
        || a.services
            .iter()
            .map(|s| s.registry_id)
            .ne(b.services.iter().map(|s| s.registry_id))
    {
        return Err("Keyboard attachment or services changed. The saved baseline cannot be reused. Load the saved draft and archive the old backup before applying to this attachment".into());
    }
    Ok(())
}
fn backup_snapshot(backup: &Backup) -> Snapshot {
    Snapshot {
        attachment: backup.attachment.clone(),
        services: backup
            .services
            .iter()
            .map(|s| Service {
                registry_id: s.registry_id,
                map: s.applied.clone(),
            })
            .collect(),
    }
}
fn normalize_backup(backup: &mut Backup) -> Result<(), String> {
    native_map(&backup.requested)?;
    backup.services.sort_by_key(|s| s.registry_id);
    for service in &mut backup.services {
        service.baseline = canonical(&service.baseline)?;
        service.applied = canonical(&service.applied)?;
    }
    normalize_snapshot(&mut backup_snapshot(backup))
}
fn normalize_record(record: &mut Record) -> Result<(), String> {
    match record {
        Record::Applied(backup) => normalize_backup(backup),
        Record::Legacy { baseline, applied } => {
            *baseline = canonical(baseline)?;
            *applied = canonical(applied)?;
            Ok(())
        }
        Record::Pending(pending) => {
            normalize_snapshot(&mut pending.before)?;
            normalize_snapshot(&mut pending.after)?;
            same_attachment(&pending.before, &pending.after)?;
            for backup in [&mut pending.previous, &mut pending.next]
                .into_iter()
                .flatten()
            {
                normalize_backup(backup)?;
                same_attachment(&pending.before, &backup_snapshot(backup))?;
            }
            Ok(())
        }
    }
}
fn load(store: &impl Store) -> Result<Option<Record>, String> {
    let mut record = store.load()?;
    if let Some(record) = &mut record {
        normalize_record(record)?;
    }
    Ok(record)
}
fn snapshot(backend: &mut impl Backend) -> Result<Snapshot, String> {
    let mut snapshot = backend.snapshot()?;
    normalize_snapshot(&mut snapshot)?;
    Ok(snapshot)
}
fn requested(record: &Record) -> Result<Vec<Remap>, String> {
    match record {
        Record::Applied(b) => Ok(b.requested.clone()),
        Record::Legacy { applied, .. } => editor_map(applied),
        Record::Pending(p) => Ok(p
            .next
            .as_ref()
            .or(p.previous.as_ref())
            .map(|b| b.requested.clone())
            .unwrap_or_default()),
    }
}
fn into_backup(record: Record, current: &Snapshot) -> Result<Backup, String> {
    let backup = match record {
        Record::Applied(backup) => backup,
        Record::Legacy { baseline, applied } => {
            if current.services.len() != 1 || current.services[0].map != applied {
                return Err("The old Mac backup has no service identity. Migration needs exactly one service matching its applied map; the backup was preserved".into());
            }
            Backup {
                attachment: current.attachment.clone(),
                requested: editor_map(&applied)?,
                services: vec![ServiceBackup {
                    registry_id: current.services[0].registry_id,
                    baseline,
                    applied,
                }],
            }
        }
        Record::Pending(_) => {
            return Err("Mac mapping recovery pending; use Undo app changes first".into())
        }
    };
    same_attachment(current, &backup_snapshot(&backup))?;
    Ok(backup)
}

// A three-way merge removes only changes the app owns. External edits to an
// owned source are conflicts; unrelated additions and removals are preserved.
fn unapply(current: &[NativeMapping], old: &ServiceBackup) -> Result<Vec<NativeMapping>, String> {
    let mut base = current.to_vec();
    let sources: BTreeSet<_> = old
        .baseline
        .iter()
        .chain(&old.applied)
        .map(|m| m.source)
        .collect();
    for source in sources {
        let baseline = old.baseline.iter().find(|m| m.source == source);
        let applied = old.applied.iter().find(|m| m.source == source);
        if baseline == applied {
            continue;
        }
        if current.iter().find(|m| m.source == source) != applied {
            return Err(format!(
                "Another tool changed mapping source {source:#x} on service {:#x}; no changes made",
                old.registry_id
            ));
        }
        base.retain(|m| m.source != source);
        if let Some(original) = baseline {
            base.push(original.clone());
        }
    }
    canonical(&base)
}
fn merge(base: &[NativeMapping], desired: &[NativeMapping]) -> Result<Vec<NativeMapping>, String> {
    let mut result = base.to_vec();
    for entry in desired {
        if let Some(existing) = result.iter().find(|m| m.source == entry.source) {
            if existing != entry {
                return Err(format!(
                    "Another tool already remaps source {:#x}; its mapping was preserved",
                    entry.source
                ));
            }
        } else {
            result.push(entry.clone());
        }
    }
    canonical(&result)
}
fn rollback(backend: &mut impl Backend, pending: &Pending) -> Result<(), String> {
    // These are the same held handles used for Apply. Native read/write checks
    // each handle's ancestry again. A missing service must not prevent us from
    // restoring the other services that are still attached.
    let mut errors = Vec::new();
    for (before, after) in pending
        .before
        .services
        .iter()
        .zip(&pending.after.services)
        .rev()
    {
        let restore = (|| {
            let map = canonical(&backend.read(before.registry_id)?)?;
            if map == before.map {
                return Ok(());
            }
            if map != after.map {
                return Err("unexpected map; preserved for manual review".into());
            }
            backend.write(before.registry_id, &before.map)?;
            if canonical(&backend.read(before.registry_id)?)? != before.map {
                return Err("rollback read-back mismatch".into());
            }
            Ok::<(), String>(())
        })();
        if let Err(e) = restore {
            errors.push(format!("service {:#x}: {e}", before.registry_id));
        }
    }
    if errors.is_empty() && snapshot(backend)? == pending.before {
        return Ok(());
    }
    Err(format!(
        "Recovery incomplete; backup retained. {}",
        errors.join("; ")
    ))
}
fn recover(
    backend: &mut impl Backend,
    store: &mut impl Store,
    pending: &Pending,
) -> Result<(), String> {
    rollback(backend, pending)?;
    store
        .save(pending.previous.clone().map(Record::Applied))
        .map_err(|e| {
            format!(
                "Maps restored, but recovery record could not be finalized; use Undo again: {e}"
            )
        })
}
fn change(
    backend: &mut impl Backend,
    store: &mut impl Store,
    remaps: Vec<Remap>,
    restore: bool,
) -> Result<Vec<Remap>, String> {
    let desired = if restore {
        Vec::new()
    } else {
        native_map(&remaps)?
    };
    let current = snapshot(backend)?;
    let record = load(store)?;
    if let Some(Record::Pending(pending)) = &record {
        if !restore {
            return Err(
                "An interrupted Mac mapping transaction needs recovery. Use Undo app changes first"
                    .into(),
            );
        }
        same_attachment(&current, &pending.before)?;
        recover(backend, store, pending)?;
        return Ok(pending
            .previous
            .as_ref()
            .map(|b| b.requested.clone())
            .unwrap_or_default());
    }
    let previous = record.map(|r| into_backup(r, &current)).transpose()?;
    if restore && previous.is_none() {
        return Err("No mappings applied by this app to undo".into());
    }
    let mut services = Vec::new();
    let mut after = current.clone();
    for (i, service) in current.services.iter().enumerate() {
        let baseline = if let Some(old) = &previous {
            unapply(&service.map, &old.services[i])?
        } else {
            service.map.clone()
        };
        let applied = if restore {
            baseline.clone()
        } else {
            merge(&baseline, &desired)?
        };
        after.services[i].map = applied.clone();
        services.push(ServiceBackup {
            registry_id: service.registry_id,
            baseline,
            applied,
        });
    }
    let next = (!restore).then(|| Backup {
        attachment: current.attachment.clone(),
        requested: remaps.clone(),
        services,
    });
    let pending = Pending {
        before: current,
        after,
        previous,
        next,
    };
    // Even an Undo has a journal: a failed file deletion or partial restore
    // must remain recoverable. Persistence always precedes host mutations.
    store.save(Some(Record::Pending(pending.clone())))?;
    let result = (|| {
        for (before, after) in pending.before.services.iter().zip(&pending.after.services) {
            if before.map == after.map {
                continue;
            }
            if canonical(&backend.read(before.registry_id)?)? != before.map {
                return Err(format!(
                    "Service {:#x} changed after preflight",
                    before.registry_id
                ));
            }
            backend.write(after.registry_id, &after.map)?;
            if canonical(&backend.read(after.registry_id)?)? != after.map {
                return Err(format!(
                    "Service {:#x} read-back did not match",
                    after.registry_id
                ));
            }
        }
        if snapshot(backend)? != pending.after {
            return Err("Services changed during the transaction".into());
        }
        store.save(pending.next.clone().map(Record::Applied))
    })();
    if let Err(error) = result {
        return Err(match recover(backend, store, &pending) {
            Ok(()) => format!("{error}; previous maps restored"),
            Err(recovery) => format!("{error}; {recovery}"),
        });
    }
    Ok(if restore { Vec::new() } else { remaps })
}

pub(super) fn get(dir: &Path) -> Result<Vec<Remap>, String> {
    let _lock = LOCK.lock().map_err(|_| "Mac mapping lock unavailable")?;
    let current = snapshot(&mut native::Native::open()?)?;
    if let Some(record) = load(&FileStore(dir))? {
        let backup = into_backup(record, &current)?;
        for (service, old) in current.services.iter().zip(&backup.services) {
            unapply(&service.map, old)?;
        }
        return Ok(backup.requested);
    }
    if current
        .services
        .iter()
        .any(|s| s.map != current.services[0].map)
    {
        return Err("Keyboard services have different existing mappings. Start a new app draft; their separate mappings will be preserved".into());
    }
    editor_map(&current.services[0].map)
}
pub(super) fn saved(dir: &Path) -> Result<Vec<Remap>, String> {
    let _lock = LOCK.lock().map_err(|_| "Mac mapping lock unavailable")?;
    requested(&load(&FileStore(dir))?.ok_or("No saved Mac mappings")?)
}
pub(super) fn apply(dir: &Path, remaps: Vec<Remap>, restore: bool) -> Result<Vec<Remap>, String> {
    let _lock = LOCK.lock().map_err(|_| "Mac mapping lock unavailable")?;
    change(
        &mut native::Native::open()?,
        &mut FileStore(dir),
        remaps,
        restore,
    )
}

#[cfg(test)]
mod tests;
