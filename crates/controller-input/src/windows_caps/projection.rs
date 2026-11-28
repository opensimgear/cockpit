use super::*;

fn range(is_range: bool, minimum: u16, maximum: u16) -> IndexRange {
    IndexRange {
        is_range,
        minimum,
        maximum: if is_range { maximum } else { minimum },
    }
}
macro_rules! common {
    ($row:expr) => {{
        let row = $row;
        let r = unsafe { row.Anonymous.Range };
        Capability {
            usage_page: row.UsagePage,
            report_id: row.ReportID,
            is_alias: row.IsAlias,
            bit_field: row.BitField,
            link_collection: row.LinkCollection,
            link_usage: row.LinkUsage,
            link_usage_page: row.LinkUsagePage,
            is_absolute: row.IsAbsolute,
            usages: range(row.IsRange, r.UsageMin, r.UsageMax),
            strings: range(row.IsStringRange, r.StringMin, r.StringMax),
            designators: range(row.IsDesignatorRange, r.DesignatorMin, r.DesignatorMax),
            data_indices: range(row.IsRange, r.DataIndexMin, r.DataIndexMax),
        }
    }};
}
pub(super) fn button(row: &HIDP_BUTTON_CAPS, count_supported: bool) -> Button {
    // All union members are u16. String/designator flags independently choose
    // their range form, even when IsRange is false for usages/data indices.
    Button {
        common: common!(row),
        report_count: count_supported.then_some(row.ReportCount),
    }
}
pub(super) fn value(row: &HIDP_VALUE_CAPS) -> Value {
    Value {
        common: common!(row),
        bit_size: row.BitSize,
        report_count: row.ReportCount,
        has_null: row.HasNull,
        units: row.Units,
        units_exp_raw: row.UnitsExp,
        unit_exponent: ((row.UnitsExp & 15) as i8) << 4 >> 4,
        logical_min_raw: row.LogicalMin,
        logical_max_raw: row.LogicalMax,
        physical_min_raw: row.PhysicalMin,
        physical_max_raw: row.PhysicalMax,
        logical_min: i64::from(row.LogicalMin),
        logical_max: maximum(row.LogicalMin, row.LogicalMax),
        physical_min: i64::from(row.PhysicalMin),
        physical_max: maximum(row.PhysicalMin, row.PhysicalMax),
    }
}
fn maximum(minimum: i32, maximum: i32) -> i64 {
    if minimum < 0 {
        i64::from(maximum)
    } else {
        i64::from(maximum as u32)
    }
}
pub(super) fn valid(row: &Capability) -> bool {
    [
        &row.usages,
        &row.strings,
        &row.designators,
        &row.data_indices,
    ]
    .iter()
    .all(|range| range.minimum <= range.maximum)
}
pub(super) fn link(index: usize, row: &HIDP_LINK_COLLECTION_NODE) -> Link {
    Link {
        index: index as u16,
        usage_page: row.LinkUsagePage,
        usage: row.LinkUsage,
        parent: row.Parent,
        number_of_children: row.NumberOfChildren,
        next_sibling: row.NextSibling,
        first_child: row.FirstChild,
        collection_type: row._bitfield as u8,
        is_alias: row._bitfield & 0x100 != 0,
        raw_flags: row._bitfield,
    }
}
pub(super) fn validate_links(snapshot: &mut Snapshot) {
    let n = snapshot.links.len();
    if snapshot.links.iter().any(|row| {
        [row.parent, row.first_child, row.next_sibling]
            .iter()
            .any(|index| *index as usize >= n)
            || row.number_of_children as usize > n
    }) {
        snapshot.fail(
            "link-indices",
            None,
            None,
            "Collection node has out-of-bounds hierarchy indices/count",
        );
        return;
    }
    for sibling in [false, true] {
        let mut colors = vec![0u8; n];
        for start in 1..n {
            let mut index = start;
            let mut path = Vec::new();
            while index != 0 && colors[index] == 0 {
                colors[index] = 1;
                path.push(index);
                index = if sibling {
                    snapshot.links[index].next_sibling
                } else {
                    snapshot.links[index].parent
                } as usize;
            }
            if index != 0 && colors[index] == 1 {
                snapshot.fail(
                    "link-cycle",
                    None,
                    None,
                    "Collection hierarchy contains a cycle",
                );
                return;
            }
            for index in path {
                colors[index] = 2;
            }
        }
    }
    for parent in 0..n {
        let mut child = snapshot.links[parent].first_child as usize;
        let mut count = 0;
        while child != 0 {
            if snapshot.links[child].parent as usize != parent {
                snapshot.fail(
                    "link-parent",
                    None,
                    None,
                    "Child collection disagrees with its parent link",
                );
                return;
            }
            count += 1;
            child = snapshot.links[child].next_sibling as usize;
        }
        if count != snapshot.links[parent].number_of_children as usize {
            snapshot.fail(
                "link-children",
                None,
                None,
                "Child list differs from advertised child count",
            );
            return;
        }
    }
    if snapshot
        .reports
        .iter()
        .flat_map(|r| {
            r.button_caps
                .iter()
                .map(|c| &c.common)
                .chain(r.value_caps.iter().map(|c| &c.common))
        })
        .any(|c| c.link_collection as usize >= n)
    {
        snapshot.fail(
            "capability-link",
            None,
            None,
            "Capability references an unavailable collection index",
        );
    }
}
