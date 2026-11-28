use crate::model::*;

#[derive(Clone, Copy, Debug)]
struct RawUsage {
    page: Option<u16>,
    id: u16,
}
impl RawUsage {
    fn resolve(self, page: u16) -> Usage {
        Usage {
            page: self.page.unwrap_or(page),
            id: self.id,
        }
    }
}

#[derive(Default)]
struct Group {
    usages: Vec<LocalIndex<RawUsage>>,
    strings: Vec<LocalIndex<u32>>,
    designators: Vec<LocalIndex<u32>>,
    usage_min: Option<usize>,
    string_min: Option<usize>,
    designator_min: Option<usize>,
}
fn add<T: Copy>(
    list: &mut Vec<LocalIndex<T>>,
    pending: &mut Option<usize>,
    action: u8,
    value: T,
) -> Result<(), &'static str> {
    match action {
        0 => list.push(LocalIndex::Single(value)),
        1 => {
            if pending.is_some() {
                return Err("Repeated local range minimum before maximum");
            }
            *pending = Some(list.len());
            list.push(LocalIndex::Single(value));
        }
        _ => {
            let index = pending.take().ok_or("Local range maximum has no minimum")?;
            let LocalIndex::Single(minimum) = list[index] else {
                unreachable!()
            };
            list[index] = LocalIndex::Range {
                minimum,
                maximum: value,
            };
        }
    }
    Ok(())
}
impl Group {
    fn empty(&self) -> bool {
        self.usages.is_empty() && self.strings.is_empty() && self.designators.is_empty()
    }
    fn finish(self, page: u16, delimited: bool, errors: &mut Vec<&'static str>) -> Locals {
        if self.usage_min.is_some() || self.string_min.is_some() || self.designator_min.is_some() {
            errors.push("Local range minimum has no maximum");
        }
        fn check<T: PartialOrd>(minimum: &T, maximum: &T, errors: &mut Vec<&'static str>) {
            if minimum > maximum {
                errors.push("Local range is reversed");
            }
        }
        for list in [&self.strings, &self.designators] {
            for row in list {
                if let LocalIndex::Range { minimum, maximum } = row {
                    check(minimum, maximum, errors);
                }
            }
        }
        let usages = self
            .usages
            .into_iter()
            .map(|row| match row {
                LocalIndex::Single(value) => LocalIndex::Single(value.resolve(page)),
                LocalIndex::Range { minimum, maximum } => {
                    let minimum = minimum.resolve(page);
                    let maximum = maximum.resolve(page);
                    if minimum.page != maximum.page {
                        errors.push("Usage range spans different usage pages");
                    }
                    check(&minimum.id, &maximum.id, errors);
                    LocalIndex::Range { minimum, maximum }
                }
            })
            .collect();
        Locals {
            delimited,
            control_index: None,
            usages,
            strings: self.strings,
            designators: self.designators,
        }
    }
}

pub(super) struct LocalState {
    groups: Vec<Group>,
    active: usize,
    open: bool,
    order: Vec<Slot>,
}
enum Slot {
    Ordinary(usize),
    Delimited(usize),
}
impl Default for LocalState {
    fn default() -> Self {
        Self {
            groups: vec![Group::default()],
            active: 0,
            open: false,
            order: Vec::new(),
        }
    }
}
impl LocalState {
    pub fn has_items(&self) -> bool {
        self.groups.len() > 1 || !self.groups[0].empty()
    }
    pub fn delimited(&self) -> bool {
        self.groups.len() > 1
    }
    pub fn apply(&mut self, tag: u8, value: u32, width: usize) -> Result<(), &'static str> {
        if tag == 10 {
            return match value {
                1 if !self.open => {
                    self.groups.push(Group::default());
                    self.active = self.groups.len() - 1;
                    self.order.push(Slot::Delimited(self.active - 1));
                    self.open = true;
                    Ok(())
                }
                0 if self.open => {
                    self.open = false;
                    self.active = 0;
                    Ok(())
                }
                0 => Err("Delimiter close has no open set"),
                1 => Err("Nested delimiter sets are forbidden"),
                _ => Err("Delimiter must be zero or one"),
            };
        }
        if self.open && !matches!(tag, 0..=2) {
            return Err("Delimited sets may contain only usage locals");
        }
        let group = &mut self.groups[self.active];
        let old_count = group.usages.len();
        let result = match tag {
            0..=2 => add(
                &mut group.usages,
                &mut group.usage_min,
                tag,
                RawUsage {
                    page: (width == 4).then_some((value >> 16) as u16),
                    id: value as u16,
                },
            ),
            3..=5 => add(
                &mut group.designators,
                &mut group.designator_min,
                tag - 3,
                value,
            ),
            7..=9 => add(&mut group.strings, &mut group.string_min, tag - 7, value),
            _ => Err("Reserved local item retained without interpretation"),
        };
        if self.active == 0 && group.usages.len() > old_count {
            self.order.push(Slot::Ordinary(old_count));
        }
        result
    }
    pub fn finish(
        self,
        page: u16,
        report_count: Option<usize>,
    ) -> (Locals, Vec<Locals>, Vec<&'static str>) {
        let mut errors = Vec::new();
        if self.open {
            errors.push("Unclosed delimiter set at Main item or end of descriptor");
        }
        // No eager Usage Minimum/Maximum expansion or repeated local snapshots.
        // Empty delimiter groups stay present; the first group is preferred only
        // when no outside primary locals were supplied.
        let mut groups = self.groups.into_iter();
        let outside = groups.next().unwrap();
        let mut alternatives: Vec<_> = groups.map(|g| g.finish(page, true, &mut errors)).collect();
        let mut primary = outside.finish(page, false, &mut errors);
        let mut next = 0usize;
        let mut ordinary_count = 0usize;
        for slot in self.order {
            let count = match slot {
                Slot::Ordinary(index) => {
                    let count = match primary.usages[index] {
                        LocalIndex::Single(_) => 1,
                        LocalIndex::Range { minimum, maximum } => usize::from(maximum.id)
                            .checked_sub(usize::from(minimum.id))
                            .and_then(|n| n.checked_add(1))
                            .unwrap_or(0),
                    };
                    let Some(total) = ordinary_count.checked_add(count) else {
                        errors.push("Local control slot index overflow");
                        break;
                    };
                    ordinary_count = total;
                    count
                }
                Slot::Delimited(index) => {
                    alternatives[index].control_index = Some(next);
                    1
                }
            };
            let Some(end) = next.checked_add(count) else {
                errors.push("Local control slot index overflow");
                break;
            };
            next = end;
        }
        // Preserve the existing one-control preferred-outside + alias-set form;
        // otherwise each declared delimiter set consumes its own ordered slot.
        if report_count == Some(1) && ordinary_count == 1 && alternatives.len() == 1 {
            alternatives[0].control_index = Some(0);
        }
        if primary.usages.is_empty() && !alternatives.is_empty() {
            let preferred = alternatives.remove(0);
            primary.usages = preferred.usages;
            primary.delimited = true;
            primary.control_index = preferred.control_index;
        }
        (primary, alternatives, errors)
    }
}
