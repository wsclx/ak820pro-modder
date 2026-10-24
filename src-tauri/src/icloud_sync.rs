//! iCloud-Drive based profile sync.
//!
//! Mirrors the app's user-mutable state (right now just the automations
//! library; more files later) into `~/Library/Mobile Documents/com~apple~
//! CloudDocs/ak820pro-modder/`. macOS's iCloud Drive daemon handles the
//! actual cross-device shuttling for free — we just have to put the files
//! there and read them back.
//!
//! # Why not the entitlement-gated `iCloud~<bundleid>` container?
//!
//! Apple's "official" iCloud container path for an app is
//! `~/Library/Mobile Documents/iCloud~io~github~wsclx~ak820pro-modder/`,
//! but reaching it needs `com.apple.developer.icloud-services`
//! entitlements *and* a provisioning profile *and* a signed app bundle.
//! For an OSS app we'd be punishing every user who builds from source.
//!
//! Instead we write to the user's plain iCloud Drive root under a
//! well-named folder. Same daemon does the same syncing, no entitlements
//! required, the user sees an obvious `ak820pro-modder/` folder in
//! Finder under iCloud Drive that they can inspect or wipe by hand.
//!
//! # Files
//!
//! In the sync folder, shared by all Macs:
//! - `automations.json`: the library as a plain JSON array, byte-for-byte
//!   the format of the local file (kept that way for older app versions,
//!   see below).
//! - `automations-tombstones.json`: `[{id, deleted_at}]` of deleted entries.
//!
//! Next to the local library, per Mac and never synced:
//! - `automations-tombstones.json`: deletions made here plus those learnt
//!   from iCloud (written by `automations::save`).
//! - `icloud-sync-base.json`: `{id: updated_at}` as of the last sync, the
//!   common ancestor for the three-way merge.
//!
//! Every file a sync overwrites keeps its previous content as `*.json.bak`.
//!
//! # Merge model
//!
//! [`sync`] merges per automation `id`; push and pull are the same
//! operation. For each id:
//! - A copy whose `updated_at` is not newer than a tombstone was deleted
//!   after its last edit and is dropped. If that copy was edited since the
//!   last sync, the deleting Mac never saw the edit: conflict instead.
//!   Without a base (first sync) the newer deletion simply wins.
//! - Present on one side only: kept. It's new, or the other side lost it
//!   without a tombstone (an older app version replaced the file). Missing
//!   without a tombstone never counts as a deletion.
//! - Present on both sides and different: the side whose `updated_at` moved
//!   since the base wins. Both moved, or neither (pre-merge files where
//!   `updated_at` is 0 everywhere): conflict.
//!
//! A conflict leaves each side's copy alone until the user picks one in
//! the SyncCard. The pick is written to both sides with `updated_at` (or
//! the tombstone's `deleted_at`) set to now, so it beats every older copy
//! and tombstone on the other Macs without asking there again.
//!
//! # Macs still running an older app version
//!
//! - It reads and writes `automations.json` as a plain array, so it keeps
//!   working; it ignores the tombstone and base files.
//! - Its push replaces the iCloud array wholesale. Entries it drops come
//!   back from the newer Macs (missing without tombstone = kept). Stale
//!   copies of deleted entries it re-adds lose against the tombstone file it
//!   left alone. Its edits carry a newer `updated_at` and merge normally.
//! - Its deletions write no tombstone, so a newer Mac adds the entry back:
//!   nothing is lost, the deletion just doesn't stick until that Mac
//!   updates.
//! - Its pull is still last-write-wins by mtime and can overwrite unsynced
//!   edits on that Mac itself, exactly as before this merge existed.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::automations::{self, Automation, Tombstone};
use crate::AppError;

/// Subdirectory of the user's iCloud Drive that holds our synced files.
/// Picked to be self-explanatory in Finder ("ak820pro-modder") rather
/// than a reverse-DNS bundle ID.
const SYNC_SUBDIR: &str = "ak820pro-modder";

/// File name inside the sync dir for the automations payload. Matches
/// the local filename exactly so manual inspection (`diff`, etc.) is
/// trivial.
const AUTOMATIONS_FILENAME: &str = "automations.json";

/// Per-Mac merge base, next to the local library. Not synced: "what did I
/// see at my last sync" is different on every Mac.
const BASE_FILENAME: &str = "icloud-sync-base.json";

/// Snapshot of the sync subsystem's view of the world. Returned by the
/// `icloud_sync_status` command so the UI can render its toggle + status
/// line without making multiple round-trips.
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    /// `true` when `~/Library/Mobile Documents/com~apple~CloudDocs/`
    /// exists on this machine. Doesn't necessarily mean iCloud Drive is
    /// signed in or syncing — only that the filesystem path is there
    /// for us to write to.
    pub icloud_available: bool,
    /// Display path to our sync subfolder, when `icloud_available`.
    /// UI shows this so the user can verify they're looking at the
    /// right place in Finder.
    pub icloud_path: Option<String>,
    /// `true` when the automations.json file exists in the iCloud
    /// folder. False on a freshly-created sync setup.
    pub remote_automations_present: bool,
    /// mtime of the remote automations.json in milliseconds since epoch.
    /// `None` when no remote copy yet.
    pub remote_automations_mtime_ms: Option<u64>,
    /// Entries waiting for the user to pick a side, from a dry-run merge,
    /// so conflicts raised by an automatic sync show up in the card.
    pub conflicts: Vec<SyncConflict>,
}

/// An automation the merge can't settle alone. `None` on a side means the
/// entry was deleted there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncConflict {
    pub id: u64,
    pub local: Option<Automation>,
    pub remote: Option<Automation>,
}

/// Which copy a conflict resolution keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Local,
    Remote,
}

/// The user's pick for one conflict. Ignored when that id is no longer in
/// conflict, so a stale click can't override a newer state.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Resolution {
    pub id: u64,
    pub keep: Side,
}

/// Outcome of one sync run.
#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    /// mtime of the iCloud automations.json afterwards, ms since epoch.
    pub remote_mtime_ms: Option<u64>,
    /// The local library changed, i.e. entries came in from iCloud.
    pub local_changed: bool,
    /// The iCloud copy changed (library or tombstones).
    pub remote_changed: bool,
    pub conflicts: Vec<SyncConflict>,
}

/// Locate the user's iCloud Drive root, if iCloud Drive is set up on
/// this machine. Doesn't probe sync state — just checks the filesystem.
pub fn detect_icloud_root() -> Option<PathBuf> {
    // We deliberately use `HOME` rather than `dirs::home_dir()`. The
    // latter would pull in a dependency for a one-line lookup; the
    // former is stable + std-only.
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join("Library/Mobile Documents/com~apple~CloudDocs");
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

fn mtime_ms(path: &Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?;
    let dur = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as u64)
}

/// Compute the snapshot using a caller-supplied iCloud root. Used by
/// the public [`status`] (with the real iCloud Drive root) and by the
/// unit tests (with a tmp dir as a fake root). Returns the "no iCloud"
/// shape when `root` is `None` — same as on a host without iCloud
/// Drive set up.
fn status_with_root(root: Option<&Path>, app_data: &Path) -> SyncStatus {
    let sync_dir = root.map(|r| r.join(SYNC_SUBDIR));
    let icloud_path = sync_dir.as_ref().map(|p| p.display().to_string());
    let remote = sync_dir.as_ref().map(|d| d.join(AUTOMATIONS_FILENAME));
    let (remote_present, remote_mtime) = match remote {
        Some(ref p) if p.exists() => (true, mtime_ms(p)),
        _ => (false, None),
    };
    // A failing dry run (unreadable file, iCloud still downloading) shows
    // no conflicts here; the next real sync reports the error.
    let conflicts = root
        .and_then(|r| sync_in(r, app_data, crate::unix_millis(), None, true).ok())
        .map(|r| r.conflicts)
        .unwrap_or_default();
    SyncStatus {
        icloud_available: root.is_some(),
        icloud_path,
        remote_automations_present: remote_present,
        remote_automations_mtime_ms: remote_mtime,
        conflicts,
    }
}

pub fn status(app_data: &Path) -> SyncStatus {
    status_with_root(detect_icloud_root().as_deref(), app_data)
}

/// Merge the local library with the iCloud copy and write the result to
/// both sides. `resolve` settles one pending conflict.
pub fn sync(app_data: &Path, resolve: Option<Resolution>) -> Result<SyncReport, AppError> {
    let root = detect_icloud_root()
        .ok_or_else(|| AppError::Protocol("iCloud Drive not detected on this machine".into()))?;
    sync_in(&root, app_data, crate::unix_millis(), resolve, false)
}

/// One side of the merge as read from disk.
struct Library {
    list: Vec<Automation>,
    tombstones: Vec<Tombstone>,
}

/// What each side should hold after the merge.
struct Merged {
    local: Vec<Automation>,
    remote: Vec<Automation>,
    /// Same set on both sides, sorted by id.
    tombstones: Vec<Tombstone>,
    base: BTreeMap<u64, i64>,
    conflicts: Vec<SyncConflict>,
}

/// Pure three-way merge, see the module docs for the rules.
fn merge(
    local: &Library,
    remote: &Library,
    base: &BTreeMap<u64, i64>,
    now: i64,
    resolve: Option<Resolution>,
) -> Merged {
    let mut tombs: HashMap<u64, i64> = HashMap::new();
    for t in local.tombstones.iter().chain(&remote.tombstones) {
        if !t.expired(now) {
            let d = tombs.entry(t.id).or_insert(t.deleted_at);
            *d = (*d).max(t.deleted_at);
        }
    }

    let mut ids: Vec<u64> = Vec::new();
    let mut seen = HashSet::new();
    let all_ids = local.list.iter().chain(&remote.list).map(|a| a.id);
    for id in all_ids.chain(tombs.keys().copied()) {
        if seen.insert(id) {
            ids.push(id);
        }
    }

    let mut local_vals: HashMap<u64, Automation> = HashMap::new();
    let mut remote_vals: HashMap<u64, Automation> = HashMap::new();
    let mut out_tombs: Vec<Tombstone> = Vec::new();
    let mut out_base = BTreeMap::new();
    let mut conflicts = Vec::new();

    for id in ids {
        let t = tombs.get(&id).copied();
        let b = base.get(&id).copied();
        let edited = |a: &Automation| b != Some(a.updated_at);
        // Apply the tombstone to one side's copy. Returns the surviving
        // copy and whether it only survived as an edit the deleter missed.
        let against_tomb = |rec: Option<&Automation>| -> (Option<Automation>, bool) {
            match (rec, t) {
                (Some(a), Some(d)) if a.updated_at <= d => {
                    if b.is_some() && edited(a) {
                        (Some(a.clone()), true)
                    } else {
                        (None, false)
                    }
                }
                (rec, _) => (rec.cloned(), false),
            }
        };
        let (l, l_unseen) = against_tomb(local.list.iter().find(|a| a.id == id));
        let (r, r_unseen) = against_tomb(remote.list.iter().find(|a| a.id == id));

        // `Some(Some(a))` = keep `a` on both sides, `Some(None)` = deleted.
        let mut decided: Option<Option<Automation>> = match (&l, &r) {
            (None, None) => Some(None),
            (Some(a), None) if !l_unseen => Some(Some(a.clone())),
            (None, Some(a)) if !r_unseen => Some(Some(a.clone())),
            (Some(a), Some(c)) if a == c => Some(Some(a.clone())),
            (Some(a), Some(c)) => match (edited(a), edited(c)) {
                (true, false) => Some(Some(a.clone())),
                (false, true) => Some(Some(c.clone())),
                _ => None,
            },
            // Edited on one side, deleted on the other.
            _ => None,
        };

        if decided.is_none() {
            if let Some(res) = resolve.filter(|res| res.id == id) {
                let chosen = match res.keep {
                    Side::Local => l.clone(),
                    Side::Remote => r.clone(),
                };
                // Stamp the pick with "now" so it outranks every older copy
                // and tombstone elsewhere and doesn't conflict again there.
                decided = Some(chosen.map(|mut a| {
                    a.updated_at = now.max(t.map_or(0, |d| d + 1));
                    a
                }));
                if decided == Some(None) {
                    out_tombs.push(Tombstone {
                        id,
                        deleted_at: now.max(t.unwrap_or(now)),
                    });
                }
            }
        } else if decided == Some(None) {
            if let Some(d) = t {
                out_tombs.push(Tombstone { id, deleted_at: d });
            }
        }

        match decided {
            Some(Some(a)) => {
                out_base.insert(id, a.updated_at);
                local_vals.insert(id, a.clone());
                remote_vals.insert(id, a);
            }
            Some(None) => {}
            None => {
                // Pending: each side keeps its copy, the base stays put.
                if let Some(d) = t {
                    out_tombs.push(Tombstone { id, deleted_at: d });
                }
                if let Some(v) = b {
                    out_base.insert(id, v);
                }
                if let Some(a) = &l {
                    local_vals.insert(id, a.clone());
                }
                if let Some(a) = &r {
                    remote_vals.insert(id, a.clone());
                }
                conflicts.push(SyncConflict {
                    id,
                    local: l,
                    remote: r,
                });
            }
        }
    }

    out_tombs.sort_by_key(|t| t.id);
    Merged {
        // Each side keeps its own order and gets the other side's new
        // entries appended, so two Macs never fight over the order.
        local: ordered(&local.list, &remote.list, &local_vals),
        remote: ordered(&remote.list, &local.list, &remote_vals),
        tombstones: out_tombs,
        base: out_base,
        conflicts,
    }
}

fn ordered(
    first: &[Automation],
    second: &[Automation],
    vals: &HashMap<u64, Automation>,
) -> Vec<Automation> {
    let mut seen = HashSet::new();
    first
        .iter()
        .chain(second)
        .filter(|a| seen.insert(a.id))
        .filter_map(|a| vals.get(&a.id).cloned())
        .collect()
}

fn read<T: serde::de::DeserializeOwned + Default>(p: &Path) -> Result<T, AppError> {
    automations::read_json(p).map_err(AppError::Protocol)
}

/// Keep the previous content as `<file>.bak`, then write atomically.
fn overwrite<T: Serialize + ?Sized>(p: &Path, value: &T) -> Result<(), AppError> {
    if p.exists() {
        let bak = p.with_extension("json.bak");
        std::fs::copy(p, &bak).map_err(|e| {
            AppError::Protocol(format!("backup {} → {}: {e}", p.display(), bak.display()))
        })?;
    }
    automations::write_json(p, value).map_err(AppError::Protocol)
}

/// iCloud Drive (before macOS 14) keeps only a `.name.icloud` placeholder
/// until a file is downloaded, typically on a freshly set-up Mac. Reading
/// that as "no remote copy" would push this Mac's list over the real one.
fn check_downloaded(p: &Path) -> Result<(), AppError> {
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    if !p.exists() && p.with_file_name(format!(".{name}.icloud")).exists() {
        return Err(AppError::Protocol(format!(
            "iCloud Drive hasn't downloaded {name} yet. Open the {SYNC_SUBDIR} folder in Finder \
             or wait a moment, then sync again."
        )));
    }
    Ok(())
}

/// [`sync`] with the iCloud root, clock and dry-run switch injected so the
/// tests stay hermetic. A dry run computes the report but writes nothing.
fn sync_in(
    root: &Path,
    app_data: &Path,
    now: i64,
    resolve: Option<Resolution>,
    dry_run: bool,
) -> Result<SyncReport, AppError> {
    // Held from the first read to the last write, dry runs included, so a
    // save or marker assignment can't land in between and get overwritten,
    // and `status` never sees the files halfway through a sync. No test:
    // provoking the interleaving needs timing tricks.
    let _lock = automations::lock();
    let dir = root.join(SYNC_SUBDIR);
    let remote_path = dir.join(AUTOMATIONS_FILENAME);
    let remote_tombs_path = dir.join(automations::TOMBSTONES_FILENAME);
    check_downloaded(&remote_path)?;
    check_downloaded(&remote_tombs_path)?;
    let local_path = automations::storage_path(app_data.to_path_buf());
    let local_tombs_path = automations::tombstones_path(app_data.to_path_buf());
    let base_path = app_data.join(BASE_FILENAME);

    let local = Library {
        list: read(&local_path)?,
        tombstones: read(&local_tombs_path)?,
    };
    let remote = Library {
        list: read(&remote_path)?,
        tombstones: read(&remote_tombs_path)?,
    };
    let base: BTreeMap<u64, i64> = read(&base_path)?;
    let merged = merge(&local, &remote, &base, now, resolve);

    let sorted = |v: &[Tombstone]| {
        let mut v = v.to_vec();
        v.sort_by_key(|t| t.id);
        v
    };
    let remote_tombs_changed = sorted(&remote.tombstones) != merged.tombstones;
    let local_tombs_changed = sorted(&local.tombstones) != merged.tombstones;
    let remote_list_changed = remote.list != merged.remote;
    let local_changed = local.list != merged.local;

    if !dry_run {
        // Remote first, base last: if we die halfway, the next run still
        // sees which side moved since the old base and finishes the job.
        // Tombstones before the list so another Mac rarely sees the
        // shortened list without the tombstones that explain it.
        if remote_tombs_changed {
            overwrite(&remote_tombs_path, &merged.tombstones)?;
        }
        if remote_list_changed {
            overwrite(&remote_path, &merged.remote)?;
        }
        if local_tombs_changed {
            overwrite(&local_tombs_path, &merged.tombstones)?;
        }
        if local_changed {
            overwrite(&local_path, &merged.local)?;
        }
        if base != merged.base {
            overwrite(&base_path, &merged.base)?;
        }
    }

    Ok(SyncReport {
        remote_mtime_ms: mtime_ms(&remote_path),
        local_changed,
        remote_changed: remote_tombs_changed || remote_list_changed,
        conflicts: merged.conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automations::AutomationKind;

    #[test]
    fn status_struct_serialises_with_expected_keys() {
        // Frontend type contract — if we ever rename a field here we
        // want the test to break before the UI does.
        let s = SyncStatus {
            icloud_available: false,
            icloud_path: None,
            remote_automations_present: false,
            remote_automations_mtime_ms: None,
            conflicts: Vec::new(),
        };
        let j = serde_json::to_value(&s).unwrap();
        assert!(j.get("icloud_available").is_some());
        assert!(j.get("icloud_path").is_some());
        assert!(j.get("remote_automations_present").is_some());
        assert!(j.get("remote_automations_mtime_ms").is_some());
        assert!(j.get("conflicts").is_some());
    }

    /// Allocate an isolated tmp dir per-test so concurrent `cargo
    /// test` runs (and reruns of the same test) don't trip over each
    /// other. Returns `(fake_icloud_root, mac_a_app_data, mac_b_app_data)`:
    /// two Macs sharing one iCloud Drive.
    fn fresh_test_dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ak820-icloud-{tag}-{pid}-{nonce}"));
        let _ = std::fs::remove_dir_all(&base);
        let icloud = base.join("fake-icloud");
        let mac_a = base.join("mac-a");
        let mac_b = base.join("mac-b");
        for d in [&icloud, &mac_a, &mac_b] {
            std::fs::create_dir_all(d).unwrap();
        }
        (icloud, mac_a, mac_b)
    }

    fn cleanup(icloud: &Path) {
        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }

    fn auto(id: u64, name: &str, updated_at: i64) -> Automation {
        Automation {
            id,
            name: name.into(),
            description: String::new(),
            kind: AutomationKind::Shell,
            payload: "true".into(),
            created_at: 1,
            updated_at,
            marker_hid: None,
        }
    }

    fn sync_mac(root: &Path, mac: &Path, now: i64, resolve: Option<Resolution>) -> SyncReport {
        sync_in(root, mac, now, resolve, false).unwrap()
    }

    fn local(mac: &Path) -> Vec<Automation> {
        automations::load(mac.to_path_buf()).unwrap()
    }

    fn remote(root: &Path) -> Vec<Automation> {
        read(&root.join(SYNC_SUBDIR).join(AUTOMATIONS_FILENAME)).unwrap()
    }

    fn remote_tombs(root: &Path) -> Vec<Tombstone> {
        read(
            &root
                .join(SYNC_SUBDIR)
                .join(automations::TOMBSTONES_FILENAME),
        )
        .unwrap()
    }

    fn save(mac: &Path, list: &[Automation]) {
        automations::save(&automations::lock(), mac.to_path_buf(), list).unwrap();
    }

    fn names(list: &[Automation]) -> Vec<&str> {
        list.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn status_reports_no_icloud_when_root_absent() {
        let (icloud, mac, _) = fresh_test_dirs("status-none");
        let s = status_with_root(None, &mac);
        assert!(!s.icloud_available);
        assert!(s.icloud_path.is_none());
        assert!(!s.remote_automations_present);
        assert!(s.remote_automations_mtime_ms.is_none());
        assert!(s.conflicts.is_empty());
        cleanup(&icloud);
    }

    #[test]
    fn status_reports_remote_when_file_present() {
        let (icloud, mac, _) = fresh_test_dirs("status-remote");
        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        std::fs::write(sync_dir.join(AUTOMATIONS_FILENAME), b"[]").unwrap();

        let s = status_with_root(Some(&icloud), &mac);
        assert!(s.icloud_available);
        assert!(s.icloud_path.is_some());
        assert!(s.remote_automations_present);
        assert!(s.remote_automations_mtime_ms.is_some());
        cleanup(&icloud);
    }

    #[test]
    fn edits_from_two_macs_merge_without_conflict() {
        let (icloud, a, b) = fresh_test_dirs("merge");
        save(&a, &[auto(1, "x1", 1), auto(2, "y1", 1)]);
        let r = sync_mac(&icloud, &a, 100, None);
        assert!(r.remote_changed && r.conflicts.is_empty());
        let r = sync_mac(&icloud, &b, 100, None);
        assert!(r.local_changed && r.conflicts.is_empty());
        assert_eq!(names(&local(&b)), ["x1", "y1"]);

        // A edits X, B edits Y, neither has seen the other's change.
        save(&a, &[auto(1, "x2", 2), auto(2, "y1", 1)]);
        save(&b, &[auto(1, "x1", 1), auto(2, "y3", 3)]);
        assert!(sync_mac(&icloud, &a, 100, None).conflicts.is_empty());
        let r = sync_mac(&icloud, &b, 100, None);
        assert!(r.conflicts.is_empty());
        assert!(r.local_changed && r.remote_changed);
        assert!(sync_mac(&icloud, &a, 100, None).conflicts.is_empty());

        assert_eq!(names(&local(&a)), ["x2", "y3"]);
        assert_eq!(local(&a), local(&b));
        assert_eq!(local(&a), remote(&icloud));

        // B's library was overwritten by the merge; the old one is kept.
        let bak: Vec<Automation> = read(&b.join("automations.json.bak")).unwrap();
        assert_eq!(names(&bak), ["x1", "y3"]);
        cleanup(&icloud);
    }

    #[test]
    fn both_macs_editing_one_entry_is_a_conflict_until_resolved() {
        let (icloud, a, b) = fresh_test_dirs("conflict");
        save(&a, &[auto(1, "x1", 1)]);
        sync_mac(&icloud, &a, 100, None);
        sync_mac(&icloud, &b, 100, None);

        save(&a, &[auto(1, "from A", 2)]);
        save(&b, &[auto(1, "from B", 3)]);
        assert!(sync_mac(&icloud, &a, 100, None).conflicts.is_empty());

        let r = sync_mac(&icloud, &b, 100, None);
        assert_eq!(r.conflicts.len(), 1);
        let c = &r.conflicts[0];
        assert_eq!(c.local.as_ref().unwrap().name, "from B");
        assert_eq!(c.remote.as_ref().unwrap().name, "from A");
        // Nothing is overwritten while the conflict is open.
        assert!(!r.local_changed && !r.remote_changed);
        assert_eq!(names(&local(&b)), ["from B"]);
        assert_eq!(names(&remote(&icloud)), ["from A"]);
        // The card sees it too.
        assert_eq!(status_with_root(Some(&icloud), &b).conflicts.len(), 1);

        let keep_b = Resolution {
            id: 1,
            keep: Side::Local,
        };
        let r = sync_mac(&icloud, &b, 500, Some(keep_b));
        assert!(r.conflicts.is_empty());
        assert_eq!(remote(&icloud), vec![auto(1, "from B", 500)]);

        // A takes B's pick without being asked again.
        let r = sync_mac(&icloud, &a, 600, None);
        assert!(r.conflicts.is_empty());
        assert_eq!(local(&a), vec![auto(1, "from B", 500)]);
        cleanup(&icloud);
    }

    #[test]
    fn deletion_spreads_and_stale_copies_stay_dead() {
        let (icloud, a, b) = fresh_test_dirs("delete");
        let now = crate::unix_millis();
        save(&a, &[auto(1, "x", 1), auto(2, "y", 1)]);
        sync_mac(&icloud, &a, now, None);
        sync_mac(&icloud, &b, now, None);

        save(&a, &[auto(2, "y", 1)]);
        let local_tombs: Vec<Tombstone> = read(&automations::tombstones_path(a.clone())).unwrap();
        assert_eq!(local_tombs.iter().map(|t| t.id).collect::<Vec<_>>(), [1]);

        sync_mac(&icloud, &a, now, None);
        assert_eq!(names(&remote(&icloud)), ["y"]);
        assert_eq!(remote_tombs(&icloud).len(), 1);

        // B still holds X but hasn't touched it: the deletion wins.
        let r = sync_mac(&icloud, &b, now, None);
        assert!(r.conflicts.is_empty());
        assert_eq!(names(&local(&b)), ["y"]);

        // An older app version pushes its stale list wholesale.
        automations::write_json(
            &icloud.join(SYNC_SUBDIR).join(AUTOMATIONS_FILENAME),
            &[auto(1, "x", 1), auto(2, "y", 1)],
        )
        .unwrap();
        let r = sync_mac(&icloud, &a, now, None);
        assert!(r.conflicts.is_empty());
        assert_eq!(names(&local(&a)), ["y"]);
        assert_eq!(names(&remote(&icloud)), ["y"]);
        cleanup(&icloud);
    }

    #[test]
    fn edit_against_deletion_is_a_conflict() {
        let (icloud, a, b) = fresh_test_dirs("edit-vs-delete");
        let now = crate::unix_millis();
        save(&a, &[auto(1, "x", 1)]);
        sync_mac(&icloud, &a, now, None);
        sync_mac(&icloud, &b, now, None);

        // B edits X before A deletes it, but A never sees the edit.
        save(&b, &[auto(1, "x edited", 2)]);
        save(&a, &[]);
        sync_mac(&icloud, &a, now, None);

        let r = sync_mac(&icloud, &b, now, None);
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.conflicts[0].local.as_ref().unwrap().name, "x edited");
        assert!(r.conflicts[0].remote.is_none());
        assert_eq!(names(&local(&b)), ["x edited"]);

        let take_icloud = Resolution {
            id: 1,
            keep: Side::Remote,
        };
        let r = sync_mac(&icloud, &b, now + 1, Some(take_icloud));
        assert!(r.conflicts.is_empty());
        assert!(local(&b).is_empty());
        assert_eq!(remote_tombs(&icloud)[0].id, 1);
        cleanup(&icloud);
    }

    #[test]
    fn tombstones_expire_after_ttl() {
        let (icloud, a, _) = fresh_test_dirs("ttl");
        let now = 1_000 * automations::TOMBSTONE_TTL_MS;
        let old = Tombstone {
            id: 1,
            deleted_at: now - automations::TOMBSTONE_TTL_MS,
        };
        let fresh = Tombstone {
            id: 2,
            deleted_at: now - 1,
        };
        automations::write_json(&automations::tombstones_path(a.clone()), &[old, fresh]).unwrap();
        // Stale copies of both deleted entries still sit in iCloud.
        automations::write_json(
            &icloud.join(SYNC_SUBDIR).join(AUTOMATIONS_FILENAME),
            &[auto(1, "expired", 1), auto(2, "deleted", 1)],
        )
        .unwrap();

        sync_mac(&icloud, &a, now, None);
        assert_eq!(remote_tombs(&icloud), vec![fresh]);
        let local_tombs: Vec<Tombstone> = read(&automations::tombstones_path(a.clone())).unwrap();
        assert_eq!(local_tombs, vec![fresh]);
        // Past its TTL a tombstone no longer blocks a stale copy.
        assert_eq!(names(&local(&a)), ["expired"]);
        cleanup(&icloud);
    }

    #[test]
    fn pre_merge_files_load_and_stay_plain_arrays() {
        let (icloud, a, _) = fresh_test_dirs("migrate");
        // Format written before this merge existed: no updated_at /
        // created_at / description / marker_hid, no tombstones, no base.
        std::fs::write(
            a.join("automations.json"),
            r#"[{"id":1,"name":"same","kind":"shell","payload":"true"},
                {"id":2,"name":"here","kind":"shell","payload":"true"},
                {"id":3,"name":"only local","kind":"shell","payload":"true"}]"#,
        )
        .unwrap();
        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        std::fs::write(
            sync_dir.join(AUTOMATIONS_FILENAME),
            r#"[{"id":1,"name":"same","kind":"shell","payload":"true"},
                {"id":2,"name":"there","kind":"shell","payload":"true"},
                {"id":4,"name":"only remote","kind":"shortcut","payload":"X"}]"#,
        )
        .unwrap();

        let r = sync_mac(&icloud, &a, 100, None);
        // Same id, different content, no way to tell who's newer: ask.
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.conflicts[0].id, 2);
        assert_eq!(
            names(&local(&a)),
            ["same", "here", "only local", "only remote"]
        );

        // The iCloud copy is still a plain array an older version can read.
        let raw = std::fs::read_to_string(sync_dir.join(AUTOMATIONS_FILENAME)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let ids: Vec<u64> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, [1, 2, 4, 3]);
        assert_eq!(v[1]["name"], "there");
        cleanup(&icloud);
    }

    #[test]
    fn undownloaded_icloud_placeholder_blocks_the_sync() {
        let (icloud, a, _) = fresh_test_dirs("placeholder");
        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        std::fs::write(sync_dir.join(".automations.json.icloud"), b"").unwrap();
        save(&a, &[auto(1, "x", 1)]);

        assert!(sync_in(&icloud, &a, 100, None, false).is_err());
        assert!(!sync_dir.join(AUTOMATIONS_FILENAME).exists());
        cleanup(&icloud);
    }
}
