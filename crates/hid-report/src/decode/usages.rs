use crate::{LocalIndex, Locals, Usage};

struct Span {
    end: usize,
    first: Usage,
    length: usize,
}

/// Prefix-indexed declarations keep ranges compact and lookup logarithmic.
pub(super) struct Index {
    spans: Vec<Span>,
    length: usize,
    delimited: bool,
}

pub(super) struct Resolver {
    indexes: Vec<Index>,
    ordinary: Vec<usize>,
    groups: Vec<(usize, usize)>,
    controls: usize,
    single_alias: bool,
}

impl Resolver {
    pub(super) fn new(primary: &Locals, alternatives: &[Locals], count: usize) -> Option<Self> {
        let locals: Vec<_> = std::iter::once(primary)
            .chain(alternatives.iter())
            .collect();
        let indexes = locals
            .iter()
            .map(|locals| Index::new(locals))
            .collect::<Option<Vec<_>>>()?;
        let ordinary: Vec<_> = locals
            .iter()
            .enumerate()
            .filter_map(|(index, locals)| (!locals.delimited).then_some(index))
            .collect();
        let mut groups = locals
            .iter()
            .enumerate()
            .filter_map(|(index, locals)| {
                locals
                    .delimited
                    .then_some((locals.control_index.unwrap_or(0), index))
            })
            .collect::<Vec<_>>();
        groups.sort_by_key(|(position, _)| *position);
        let ordinary_count = ordinary
            .iter()
            .map(|index| indexes[*index].length)
            .max()
            .unwrap_or(0);
        let single_alias =
            count == 1 && ordinary_count == 1 && groups.len() == 1 && groups[0].0 == 0;
        let controls = ordinary_count.checked_add(groups.len())?;
        Some(Self {
            indexes,
            ordinary,
            groups,
            controls,
            single_alias,
        })
    }

    pub(super) fn visit(&self, selected: usize, variable: bool, mut emit: impl FnMut(Usage)) {
        if !variable {
            for index in &self.indexes {
                index.visit(selected, false, &mut emit);
            }
            return;
        }
        if self.single_alias {
            for &index in &self.ordinary {
                self.indexes[index].visit(0, true, &mut emit);
            }
            self.indexes[self.groups[0].1].visit(0, false, &mut emit);
            return;
        }
        let Some(last) = self.controls.checked_sub(1) else {
            return;
        };
        let selected = selected.min(last);
        let group = self
            .groups
            .partition_point(|(position, _)| *position < selected);
        if self
            .groups
            .get(group)
            .is_some_and(|(position, _)| *position == selected)
        {
            self.indexes[self.groups[group].1].visit(0, false, emit);
        } else {
            for &index in &self.ordinary {
                self.indexes[index].visit(selected - group, true, &mut emit);
            }
        }
    }
}

impl Index {
    pub(super) fn new(locals: &Locals) -> Option<Self> {
        let mut spans = Vec::with_capacity(locals.usages.len());
        let mut length = 0usize;
        for usage in &locals.usages {
            let (first, count) = match usage {
                LocalIndex::Single(usage) => (*usage, 1),
                LocalIndex::Range { minimum, maximum } => {
                    if minimum.page != maximum.page || minimum.id > maximum.id {
                        return None;
                    }
                    (*minimum, usize::from(maximum.id - minimum.id) + 1)
                }
            };
            length = length.checked_add(count)?;
            spans.push(Span {
                end: length,
                first,
                length: count,
            });
        }
        Some(Self {
            spans,
            length,
            delimited: locals.delimited,
        })
    }

    pub(super) fn at(&self, element: usize, repeat_last: bool) -> Option<Usage> {
        let element = if element >= self.length && repeat_last {
            self.length.checked_sub(1)?
        } else {
            element
        };
        let index = self.spans.partition_point(|span| span.end <= element);
        let span = self.spans.get(index)?;
        Some(Usage {
            page: span.first.page,
            id: span.first.id + u16::try_from(element - (span.end - span.length)).ok()?,
        })
    }

    pub(super) fn visit(&self, element: usize, repeat_last: bool, mut emit: impl FnMut(Usage)) {
        if self.delimited {
            if element == 0 || repeat_last {
                for span in &self.spans {
                    for offset in 0..span.length {
                        emit(Usage {
                            page: span.first.page,
                            id: span.first.id + offset as u16,
                        });
                    }
                }
            }
        } else if let Some(usage) = self.at(element, repeat_last) {
            emit(usage);
        }
    }
}

pub(super) fn attribution_cost(locals: &Locals) -> Option<usize> {
    if !locals.delimited {
        return Some(1);
    }
    locals
        .usages
        .iter()
        .try_fold(0usize, |cost, index| {
            let count = match index {
                LocalIndex::Single(_) => 1,
                LocalIndex::Range { minimum, maximum }
                    if minimum.page == maximum.page && minimum.id <= maximum.id =>
                {
                    usize::from(maximum.id - minimum.id) + 1
                }
                _ => return None,
            };
            cost.checked_add(count)
        })
        .map(|cost| cost.max(1))
}
