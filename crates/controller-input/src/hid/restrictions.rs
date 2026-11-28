use osg_hid_report::{LocalIndex, Locals, ReportKind, Usage};

fn forbidden(minimum: Usage, maximum: Usage) -> bool {
    if minimum.page != maximum.page {
        return true;
    }
    let overlaps = |first, last| minimum.id <= last && maximum.id >= first;
    match minimum.page {
        1 => overlaps(1, 2) || overlaps(6, 7) || overlaps(0x80, 0x8f),
        7 | 0x0c | 0x84 | 0x85 => true,
        _ => false,
    }
}
fn restricted(locals: &Locals) -> bool {
    locals.usages.iter().any(|usage| match usage {
        LocalIndex::Single(usage) => forbidden(*usage, *usage),
        LocalIndex::Range { minimum, maximum } => forbidden(*minimum, *maximum),
    })
}
fn sensitive_input(locals: &Locals) -> bool {
    locals.usages.iter().any(|usage| {
        let (minimum, maximum) = match usage {
            LocalIndex::Single(value) => (*value, *value),
            LocalIndex::Range { minimum, maximum } => (*minimum, *maximum),
        };
        if minimum.page != maximum.page {
            return true;
        }
        match minimum.page {
            7 | 0x0c => true,
            1 => minimum.id <= 0x8f && maximum.id >= 0x80,
            _ => false,
        }
    })
}
pub(super) fn reasons(raw: &[u8]) -> Vec<String> {
    let descriptor = match osg_hid_report::parse_descriptor(raw) {
        Ok(descriptor) if descriptor.diagnostics.is_empty() => descriptor,
        _ => return vec!["Descriptor cannot establish collection policy; metadata only".into()],
    };
    let roots: Vec<_> = descriptor
        .collections
        .iter()
        .filter(|c| c.parent.is_none())
        .collect();
    if roots.is_empty() {
        return vec!["No top-level collection identity is declared; metadata only".into()];
    }
    let mut reasons = Vec::new();
    for collection in roots {
        let groups = std::iter::once(&collection.primary).chain(collection.alternatives.iter());
        let mut found = false;
        for group in groups {
            found |= !group.usages.is_empty();
            if restricted(group) {
                reasons.push(
                    "Keyboard, mouse, pointer or system/consumer collection is metadata only"
                        .into(),
                );
                break;
            }
        }
        if !found {
            reasons.push("Top-level usage identity is unavailable; metadata only".into());
        }
    }
    // A vendor top-level usage cannot hide explicit keyboard/system input
    // fields. Numeric vendor usages and ordinary X/Y/button usages alone do
    // not establish mouse/keyboard semantics, so they remain inspectable.
    if descriptor
        .fields
        .iter()
        .filter(|field| field.kind == ReportKind::Input)
        .any(|field| {
            std::iter::once(&field.primary)
                .chain(field.alternatives.iter())
                .any(sensitive_input)
        })
    {
        reasons.push("Explicit keyboard or system/consumer input fields are metadata only, including under vendor collections".into());
    }
    if descriptor
        .collections
        .iter()
        .filter(|collection| collection.parent.is_some() && collection.type_code == 1)
        .any(|collection| {
            std::iter::once(&collection.primary)
                .chain(collection.alternatives.iter())
                .any(restricted)
        })
    {
        reasons.push(
            "Nested application collection has keyboard/mouse/system semantics; metadata only"
                .into(),
        );
    }
    reasons.sort();
    reasons.dedup();
    reasons
}
