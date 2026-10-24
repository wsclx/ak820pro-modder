//! ANSI feature-report custom RGB, recovered from the vendor 1.0.0.5 driver.
//! 44B900 selects mode 80; 44BA10 uploads nine raw 64-byte colour blocks.
//! Common UI slots are translated to this revision's one-based LED matrix.
//! Ported from vespovios/ak820pro-modder@017c18b; verified there on an ANSI
//! 0C45:800A (WASD + E). The slot table is ANSI-only.
use crate::legacy::{control_packet, send, TransactionLock};
use crate::{
    commands::per_key_rgb::{CustomLedMap, LED_COUNT},
    Error, Result,
};
use hidapi::HidDevice;

/// (common UI slot, feature-firmware light_index), from ANSI layout XML.
pub const ANSI_LED_SLOTS: &[(usize, usize)] = &[
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 8),
    (8, 9),
    (9, 10),
    (10, 11),
    (11, 12),
    (12, 13),
    (106, 119),
    (16, 19),
    (17, 20),
    (18, 21),
    (19, 22),
    (20, 23),
    (21, 24),
    (22, 25),
    (23, 26),
    (24, 27),
    (25, 28),
    (26, 29),
    (27, 30),
    (28, 31),
    (92, 103),
    (107, 117),
    (32, 37),
    (33, 38),
    (34, 39),
    (35, 40),
    (36, 41),
    (37, 42),
    (38, 43),
    (39, 44),
    (40, 45),
    (41, 46),
    (42, 47),
    (43, 48),
    (44, 49),
    (97, 67),
    (105, 118),
    (48, 55),
    (49, 56),
    (50, 57),
    (51, 58),
    (52, 59),
    (53, 60),
    (54, 61),
    (55, 62),
    (56, 63),
    (57, 64),
    (58, 65),
    (59, 66),
    (76, 85),
    (108, 121),
    (64, 73),
    (65, 74),
    (66, 75),
    (67, 76),
    (68, 77),
    (69, 78),
    (70, 79),
    (71, 80),
    (72, 81),
    (73, 82),
    (74, 83),
    (75, 84),
    (90, 101),
    (80, 91),
    (81, 92),
    (82, 93),
    (83, 94),
    (84, 95),
    (85, 96),
    (87, 98),
    (88, 99),
    (89, 100),
    (91, 102),
];

pub fn encode(map: &CustomLedMap) -> Result<[u8; 576]> {
    if map.leds.len() != LED_COUNT
        || map
            .leds
            .iter()
            .enumerate()
            .any(|(i, l)| usize::from(l.led_id) != i)
    {
        return Err(Error::UnexpectedResponse(
            "Custom RGB needs exactly 128 ordered LED entries".into(),
        ));
    }
    let mut data = [0; 576];
    for &(slot, id) in ANSI_LED_SLOTS {
        let l = &map.leds[slot];
        data[id * 4..id * 4 + 4].copy_from_slice(&[id as u8, l.red, l.green, l.blue]);
    }
    // The vendor sends 0x240 bytes; the final two are the bank delimiter.
    data[574..].copy_from_slice(&[0xaa, 0x55]);
    Ok(data)
}

fn packet(op: u8, flag: u8) -> [u8; 64] {
    control_packet(op, 0, flag.into())
}

/// Command replies echo `04 <op>` and carry status 1 in byte 3.
fn validate_ack(reply: &[u8; 64], command: u8) -> Result<()> {
    if reply[0] != 4 || reply[1] != command || reply[3] != 1 {
        return Err(Error::UnexpectedResponse(format!(
            "keyboard did not acknowledge feature command 0x{command:02x}: {}",
            hex::encode(&reply[..9])
        )));
    }
    Ok(())
}

pub(crate) fn apply(
    device: &HidDevice,
    pid: u16,
    map: &CustomLedMap,
    brightness: u8,
) -> Result<()> {
    let _guard = TransactionLock::acquire(pid)?;
    apply_with(
        |p| {
            let r = send(device, p)?;
            Ok(r[1..].try_into().unwrap())
        },
        map,
        brightness,
    )
}

fn apply_with(
    mut send: impl FnMut(&[u8; 64]) -> Result<[u8; 64]>,
    map: &CustomLedMap,
    brightness: u8,
) -> Result<()> {
    let data = encode(map)?;
    if brightness > 5 {
        return Err(Error::OutOfRange {
            field: "custom RGB brightness",
            value: brightness.into(),
            max: 5,
        });
    }
    let mut mode = [0; 64];
    mode[0] = 0x80;
    mode[9] = brightness;
    mode[14..16].copy_from_slice(&[0xaa, 0x55]);
    let result = (|| {
        for (op, flag) in [(0x18, 0), (0x13, 1)] {
            validate_ack(&send(&packet(op, flag))?, op)?;
        }
        send(&mode)?;
        validate_ack(&send(&packet(2, 0))?, 2)?;
        validate_ack(&send(&packet(0xf0, 0))?, 0xf0)?;
        for (op, flag) in [(0x18, 0), (0x23, 9)] {
            validate_ack(&send(&packet(op, flag))?, op)?;
        }
        for bytes in data.chunks_exact(64) {
            let p: &[u8; 64] = bytes.try_into().unwrap();
            // Data replies echo raw colour bytes; only command ACKs carry
            // a status field. Requiring command-shaped data ACKs is wrong.
            send(p)?;
        }
        validate_ack(&send(&packet(2, 0))?, 2)
    })();
    let finished = send(&packet(0xf0, 0)).and_then(|r| validate_ack(&r, 0xf0));
    result.and(finished)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_ansi_keys_at_vendor_indices_and_keeps_reserved_space_zero() {
        let mut m = CustomLedMap::default();
        for &(s, _) in ANSI_LED_SLOTS {
            m.set(s, 124, 92, 255);
        }
        m.set(34, 255, 0, 0);
        m.set(49, 0, 255, 0);
        m.set(50, 0, 0, 255);
        m.set(51, 255, 255, 0);
        let p = encode(&m).unwrap();
        for (id, rgb) in [
            (39, [255, 0, 0]),
            (56, [0, 255, 0]),
            (57, [0, 0, 255]),
            (58, [255, 255, 0]),
        ] {
            assert_eq!(&p[id * 4..id * 4 + 4], &[id as u8, rgb[0], rgb[1], rgb[2]]);
        }
        assert_eq!(&p[94 * 4..94 * 4 + 4], &[94, 124, 92, 255]); // Space
        assert_eq!(&p[85 * 4..85 * 4 + 4], &[85, 124, 92, 255]); // ANSI Enter
        assert_eq!(&p[512..574], &[0; 62]);
        assert_eq!(&p[574..], &[0xaa, 0x55]);
        let source: std::collections::BTreeSet<_> = ANSI_LED_SLOTS.iter().map(|(s, _)| s).collect();
        let target: std::collections::BTreeSet<_> = ANSI_LED_SLOTS.iter().map(|(_, i)| i).collect();
        assert_eq!(source.len(), 81);
        assert_eq!(target.len(), 81);
    }
    #[test]
    fn full_write_uses_two_sessions_and_nine_unframed_blocks() {
        let m = CustomLedMap::default();
        let mut packets = Vec::new();
        apply_with(
            |p| {
                packets.push(*p);
                let mut r = *p;
                r[3] = 1;
                Ok(r)
            },
            &m,
            5,
        )
        .unwrap();
        assert_eq!(packets.len(), 18);
        assert_eq!(packets[2][0], 0x80);
        assert_eq!(packets[2][9], 5);
        assert_eq!(packets[6], packet(0x23, 9));
        let body: Vec<_> = packets[7..16]
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect();
        assert_eq!(body, encode(&m).unwrap());
        assert_eq!(packets[16], packet(2, 0));
        assert_eq!(packets[17], packet(0xf0, 0));
    }
    #[test]
    fn rejects_malformed_maps_before_hardware_and_never_commits_failed_bank() {
        let mut m = CustomLedMap::default();
        m.leds.pop();
        assert!(apply_with(|_| panic!("invalid map reached hardware"), &m, 5).is_err());
        let mut count = 0;
        let mut packets = Vec::new();
        let result = apply_with(
            |p| {
                packets.push(*p);
                count += 1;
                if count == 10 {
                    return Err(Error::UnexpectedResponse("unplugged".into()));
                }
                let mut r = *p;
                r[3] = 1;
                Ok(r)
            },
            &CustomLedMap::default(),
            5,
        );
        assert!(result.is_err());
        assert_eq!(packets.last().unwrap(), &packet(0xf0, 0));
        assert_eq!(packets.iter().filter(|p| **p == packet(2, 0)).count(), 1); // mode commit only
    }
}
