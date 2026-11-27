//! PLC input and output addresses in one written form, so `%I0.0`, `i 0.0` and `I00.0` are
//! seen as the same address.

use crate::symbol::IoKind;

/// An input or output address in its canonical form: `I0.3`, `Q4.7`, `IW256`, `QW64`. Leading
/// `%`, spaces, case and leading zeros don't matter. `None` for anything else (memory, data
/// blocks, bytes, double words, a bit above 7, text).
pub fn canonical(address: &str) -> Option<String> {
    let a: String = address.trim().trim_start_matches('%').chars().filter(|c| !c.is_whitespace()).collect();
    let a = a.to_uppercase();
    let number =
        |s: &str| (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
    for word in ["IW", "QW"] {
        if let Some(rest) = a.strip_prefix(word) {
            return number(rest).map(|n| format!("{word}{n}"));
        }
    }
    for bit in ["I", "Q"] {
        if let Some(rest) = a.strip_prefix(bit) {
            let (byte, b) = rest.split_once('.')?;
            let (byte, b) = (number(byte)?, number(b)?);
            return (b < 8).then(|| format!("{bit}{byte}.{b}"));
        }
    }
    None
}

/// Whether a canonical address is the right kind for a channel: `I`/`Q` bits for digital
/// inputs/outputs, `IW`/`QW` words for analog.
pub fn fits(kind: IoKind, canonical: &str) -> bool {
    let prefix = match kind {
        IoKind::DigitalIn => "I",
        IoKind::DigitalOut => "Q",
        IoKind::AnalogIn => "IW",
        IoKind::AnalogOut => "QW",
    };
    let word = canonical.len() > 1 && canonical.as_bytes()[1] == b'W';
    canonical.starts_with(prefix) && word == matches!(kind, IoKind::AnalogIn | IoKind::AnalogOut)
}

/// Whether two addresses as typed are the same: canonically if both are inputs or outputs,
/// otherwise ignoring case, spaces and a leading `%`.
pub fn same(a: &str, b: &str) -> bool {
    match (canonical(a), canonical(b)) {
        (Some(x), Some(y)) => x == y,
        _ => {
            let loose = |s: &str| s.trim().trim_start_matches('%').replace(' ', "").to_uppercase();
            loose(a) == loose(b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_address_are_one_address() {
        for a in ["I0.0", "%I0.0", "i0.0", " % I 00.0 ", "I0.00"] {
            assert_eq!(canonical(a).as_deref(), Some("I0.0"), "{a}");
        }
        assert_eq!(canonical("%iw0256").as_deref(), Some("IW256"));
        assert_eq!(canonical("Q12.7").as_deref(), Some("Q12.7"));
        for not in ["", "I0.8", "I.0", "I0.", "M0.0", "MW10", "IB0", "ID4", "DB1.DBX0.0", "IW", "I1", "I+1.0", "IW-2"] {
            assert_eq!(canonical(not), None, "{not}");
        }
        assert!(same("%I0.0", "I00.0") && same("db1.x", "DB1.X") && !same("I0.0", "I0.1"));
    }

    #[test]
    fn a_channel_takes_only_its_kind_of_address() {
        assert!(
            fits(IoKind::DigitalIn, "I0.0") && !fits(IoKind::DigitalIn, "IW256") && !fits(IoKind::DigitalIn, "Q0.0")
        );
        assert!(fits(IoKind::AnalogIn, "IW256") && !fits(IoKind::AnalogIn, "I0.0") && !fits(IoKind::AnalogIn, "QW256"));
        assert!(
            fits(IoKind::DigitalOut, "Q0.0") && fits(IoKind::AnalogOut, "QW64") && !fits(IoKind::AnalogOut, "Q0.0")
        );
    }
}

#[cfg(test)]
mod project_tests {
    use crate::{ADDRESS, Command, ModelError};

    #[test]
    fn the_same_address_written_differently_is_refused_on_a_second_channel() {
        let mut p = crate::sample::with_plc().unwrap();
        let kf1 = p.devices.values().find(|d| d.tag == "-KF1").unwrap().id;
        let channels = p.plc_channels_of(kf1);
        assert_eq!(channels[0].address, "I0.0");
        let other = &channels[1];
        let mut properties = p.function(other.function).unwrap().properties.clone();
        properties.insert(ADDRESS.into(), "%i00.0".into());
        let e = p.apply(Command::SetProperties { id: other.function, properties }).unwrap_err();
        assert!(matches!(e, ModelError::DuplicateAddress(..)), "{e}");
    }
}
