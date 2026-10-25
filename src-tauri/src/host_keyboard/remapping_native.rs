//! IOHID service handles stay attached to their event-system client. Each
//! service is resolved through IORegistry to its physical USB device ancestor.
use super::{canonical, Attachment, Backend, NativeMapping, Service, Snapshot};
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFType, CFTypeRef, TCFType},
    dictionary::{CFDictionary, CFMutableDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use std::{
    collections::BTreeSet,
    ffi::{c_char, c_void},
};

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreateSimpleClient(allocator: *const c_void) -> CFTypeRef;
    fn IOHIDEventSystemClientCopyServices(client: CFTypeRef) -> CFArrayRef;
    fn IOHIDServiceClientCopyProperty(service: CFTypeRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDServiceClientSetProperty(service: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> u8;
    fn IOHIDServiceClientConformsTo(service: CFTypeRef, page: u32, usage: u32) -> i32;
    fn IOHIDServiceClientGetRegistryID(service: CFTypeRef) -> CFTypeRef;
    fn IORegistryEntryIDMatching(id: u64) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingService(port: u32, matching: CFMutableDictionaryRef) -> u32;
    fn IORegistryEntryGetParentEntry(entry: u32, plane: *const c_char, parent: *mut u32) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: CFStringRef,
        allocator: *const c_void,
        options: u32,
    ) -> CFTypeRef;
    fn IOObjectConformsTo(object: u32, class: *const c_char) -> u8;
    fn IOObjectRelease(object: u32) -> i32;
}
struct Entry(u32);
impl Drop for Entry {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}
fn owned(value: CFTypeRef) -> Option<CFType> {
    (!value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}
fn property(service: &CFType, key: &str) -> Option<CFType> {
    owned(unsafe {
        IOHIDServiceClientCopyProperty(
            service.as_CFTypeRef(),
            CFString::new(key).as_concrete_TypeRef(),
        )
    })
}
fn registry_property(entry: &Entry, key: &str) -> Option<CFType> {
    owned(unsafe {
        IORegistryEntryCreateCFProperty(
            entry.0,
            CFString::new(key).as_concrete_TypeRef(),
            std::ptr::null(),
            0,
        )
    })
}
fn number(value: Option<CFType>) -> Option<u64> {
    value?
        .downcast::<CFNumber>()?
        .to_i64()
        .and_then(|n| u64::try_from(n).ok())
}
fn string(value: Option<CFType>) -> Option<String> {
    Some(value?.downcast::<CFString>()?.to_string())
}
fn target(
    vendor: Option<u64>,
    product: Option<u64>,
    transport: Option<&str>,
    name: Option<&str>,
    keyboard: bool,
) -> bool {
    vendor == Some(0x0c45)
        && product == Some(0x800a)
        && transport == Some("USB")
        && name.is_some_and(|s| s.starts_with("AK820"))
        && keyboard
}
fn usb_parent(id: u64) -> Result<u64, String> {
    let matching = unsafe { IORegistryEntryIDMatching(id) };
    if matching.is_null() {
        return Err("Cannot create registry match".into());
    }
    // IOServiceGetMatchingService consumes the matching dictionary.
    let mut entry = Entry(unsafe { IOServiceGetMatchingService(0, matching) });
    if entry.0 == 0 {
        return Err("Keyboard service disconnected".into());
    }
    // The IOService parent chain is acyclic; bound traversal defensively.
    for _ in 0..64 {
        if unsafe {
            IOObjectConformsTo(entry.0, c"IOUSBHostDevice".as_ptr()) != 0
                || IOObjectConformsTo(entry.0, c"IOUSBDevice".as_ptr()) != 0
        } {
            if !target(
                number(registry_property(&entry, "idVendor")),
                number(registry_property(&entry, "idProduct")),
                Some("USB"),
                string(registry_property(&entry, "USB Product Name")).as_deref(),
                true,
            ) {
                return Err("Keyboard service has an unexpected physical USB parent".into());
            }
            let mut parent_id = 0;
            if unsafe { IORegistryEntryGetRegistryEntryID(entry.0, &mut parent_id) } != 0
                || parent_id == 0
            {
                return Err("Cannot identify physical USB parent".into());
            }
            return Ok(parent_id);
        }
        let mut parent = 0;
        if unsafe { IORegistryEntryGetParentEntry(entry.0, c"IOService".as_ptr(), &mut parent) }
            != 0
            || parent == 0
        {
            break;
        }
        entry = Entry(parent);
    }
    Err("Cannot establish physical USB ancestry; no mappings changed".into())
}
fn one_parent(parents: impl Iterator<Item = u64>) -> Result<u64, String> {
    let parents: BTreeSet<_> = parents.collect();
    if parents.is_empty() {
        return Err("AJAZZ USB keyboard not found. Connect it by USB first".into());
    }
    if parents.len() != 1 || parents.contains(&0) {
        return Err("Multiple physical USB keyboards or ambiguous ancestry; leave only the target AK820 connected".into());
    }
    Ok(*parents.first().unwrap())
}
struct Handle {
    id: u64,
    parent: u64,
    value: CFType,
}
pub(super) struct Native {
    handles: Vec<Handle>,
    attachment: Attachment,
    // Services do not retain the event-system client; drop them first.
    _client: CFType,
}
impl Native {
    pub(super) fn open() -> Result<Self, String> {
        let client = owned(unsafe { IOHIDEventSystemClientCreateSimpleClient(std::ptr::null()) })
            .ok_or("Cannot open macOS HID event system")?;
        let raw = unsafe { IOHIDEventSystemClientCopyServices(client.as_CFTypeRef()) };
        if raw.is_null() {
            return Err("Cannot enumerate keyboard services".into());
        }
        let array: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(raw) };
        let mut handles = Vec::new();
        for service in array.iter() {
            // Conformance includes secondary keyboard collections whose primary
            // usage is Consumer Control. Filtering PrimaryUsage alone misses them.
            if !target(
                number(property(&service, "VendorID")),
                number(property(&service, "ProductID")),
                string(property(&service, "Transport")).as_deref(),
                string(property(&service, "Product")).as_deref(),
                unsafe { IOHIDServiceClientConformsTo(service.as_CFTypeRef(), 1, 6) != 0 },
            ) {
                continue;
            }
            let registry = unsafe { IOHIDServiceClientGetRegistryID(service.as_CFTypeRef()) };
            let id = if registry.is_null() {
                None
            } else {
                number(Some(unsafe { CFType::wrap_under_get_rule(registry) }))
            }
            .ok_or("Missing keyboard service registry ID")?;
            handles.push(Handle {
                id,
                parent: usb_parent(id)?,
                value: (*service).clone(),
            });
        }
        handles.sort_by_key(|h| h.id);
        let parent = one_parent(handles.iter().map(|h| h.parent))?;
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.bootsessionuuid"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("Cannot identify this macOS boot session".into());
        }
        let boot_session = String::from_utf8(out.stdout)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned();
        if boot_session.is_empty() {
            return Err("Missing macOS boot session identity".into());
        }
        Ok(Self {
            handles,
            attachment: Attachment {
                boot_session,
                usb_registry_id: parent,
            },
            _client: client,
        })
    }
    fn handle(&self, id: u64) -> Result<&Handle, String> {
        let handle = self
            .handles
            .iter()
            .find(|h| h.id == id)
            .ok_or("Unknown keyboard service")?;
        if usb_parent(id)? != handle.parent {
            return Err("Keyboard attachment changed".into());
        }
        Ok(handle)
    }
}
fn read(handle: &Handle) -> Result<Vec<NativeMapping>, String> {
    let Some(value) = property(&handle.value, "UserKeyMapping") else {
        return Ok(vec![]);
    };
    let array = value
        .downcast::<CFArray>()
        .ok_or("Invalid native mapping array")?;
    let mut map = Vec::new();
    for raw in array.iter() {
        let value = unsafe { CFType::wrap_under_get_rule(*raw) };
        let dict = value
            .downcast::<CFDictionary>()
            .ok_or("Invalid native mapping entry")?;
        let field = |key: &str| {
            let key = CFString::new(key);
            let value = *dict
                .find(key.as_CFTypeRef())
                .ok_or("Missing native mapping usage")?;
            number(Some(unsafe { CFType::wrap_under_get_rule(value) }))
                .ok_or("Invalid native mapping usage")
        };
        map.push(NativeMapping {
            source: field("HIDKeyboardModifierMappingSrc")?,
            destination: field("HIDKeyboardModifierMappingDst")?,
        });
    }
    canonical(&map)
}
impl Backend for Native {
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        let services = self
            .handles
            .iter()
            .map(|h| {
                Ok(Service {
                    registry_id: h.id,
                    map: read(self.handle(h.id)?)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Snapshot {
            attachment: self.attachment.clone(),
            services,
        })
    }
    fn read(&mut self, id: u64) -> Result<Vec<NativeMapping>, String> {
        read(self.handle(id)?)
    }
    fn write(&mut self, id: u64, map: &[NativeMapping]) -> Result<(), String> {
        let handle = self.handle(id)?;
        let map = canonical(map)?;
        let entries: Vec<_> = map
            .iter()
            .map(|m| {
                CFDictionary::from_CFType_pairs(&[
                    (
                        CFString::new("HIDKeyboardModifierMappingSrc"),
                        CFNumber::from(m.source as i64),
                    ),
                    (
                        CFString::new("HIDKeyboardModifierMappingDst"),
                        CFNumber::from(m.destination as i64),
                    ),
                ])
            })
            .collect();
        let array = CFArray::from_CFTypes(&entries);
        if unsafe {
            IOHIDServiceClientSetProperty(
                handle.value.as_CFTypeRef(),
                CFString::new("UserKeyMapping").as_concrete_TypeRef(),
                array.as_CFTypeRef(),
            )
        } == 0
        {
            return Err(format!("macOS rejected the mapping for service {id:#x}. Check Input Monitoring if requested"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_services_by_usb_parent_and_filters_unrelated_devices() {
        assert_eq!(one_parent([42, 42].into_iter()).unwrap(), 42);
        assert!(one_parent([42, 43].into_iter()).is_err());
        assert!(one_parent([0, 0].into_iter()).is_err());
        assert!(one_parent([].into_iter()).is_err());
        assert!(target(
            Some(0x0c45),
            Some(0x800a),
            Some("USB"),
            Some("AK820"),
            true
        ));
        for (v, p, t, n, k) in [
            (Some(1), Some(0x800a), Some("USB"), Some("AK820"), true),
            (Some(0x0c45), Some(1), Some("USB"), Some("AK820"), true),
            (
                Some(0x0c45),
                Some(0x800a),
                Some("Bluetooth"),
                Some("AK820"),
                true,
            ),
            (Some(0x0c45), Some(0x800a), Some("USB"), Some("Other"), true),
            (
                Some(0x0c45),
                Some(0x800a),
                Some("USB"),
                Some("AK820"),
                false,
            ),
        ] {
            assert!(!target(v, p, t, n, k));
        }
    }
    #[test]
    #[ignore = "read-only: requires a connected USB AK820; prints service ancestry and maps"]
    fn inspect_usb_services_read_only() {
        let snapshot = Native::open().unwrap().snapshot().unwrap();
        eprintln!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
        assert!(!snapshot.services.is_empty());
    }
}
