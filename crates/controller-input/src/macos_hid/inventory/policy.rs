use serde_json::Value;

pub(super) fn native_restricted(page: u64, usage: u64) -> bool {
    matches!(page, 7 | 0x0c | 0x84 | 0x85)
        || page == 1 && (matches!(usage, 1 | 2 | 6 | 7) || (0x80..=0x8f).contains(&usage))
}

fn element_restricted(element: &Value, by_cookie: &std::collections::HashMap<u64, &Value>) -> bool {
    let page = element["usagePage"].as_u64().unwrap_or(0);
    let usage = element["usage"].as_u64().unwrap_or(0);
    if let (Some(min), Some(max)) = (element["usageMin"].as_u64(), element["usageMax"].as_u64()) {
        if min > max || page == 1 && min <= 0x8f && max >= 0x80 {
            return true;
        }
    }
    if !native_restricted(page, usage) {
        return false;
    }
    // Joysticks commonly group axes in a nested Pointer physical collection.
    // That grouping does not establish a mouse. Require a controller application
    // ancestor, and never exempt a nested Pointer application collection.
    if page == 1
        && matches!(usage, 1 | 2)
        && element["type"] == 513
        && matches!(element["collectionType"].as_u64(), Some(0 | 2))
    {
        let mut current = element;
        for _ in 0..64 {
            let Some(parent) = current["parentCookie"]
                .as_u64()
                .and_then(|p| by_cookie.get(&p))
            else {
                return true;
            };
            if parent["type"] == 513 && parent["collectionType"] == 1 {
                return !(parent["usagePage"] == 1
                    && matches!(parent["usage"].as_u64(), Some(4 | 5 | 8)));
            }
            current = parent;
        }
    }
    true
}

pub(super) fn elements_restricted(elements: &[Value]) -> bool {
    let by_cookie = elements
        .iter()
        .filter_map(|e| e["cookie"].as_u64().map(|c| (c, e)))
        .collect();
    elements.iter().any(|e| element_restricted(e, &by_cookie))
}

// Without wire descriptor evidence, accept only a complete controller tree.
// Vendor-only or unparented fields cannot establish safe collection semantics.
pub(super) fn element_fallback(elements: &[Value]) -> bool {
    let by_cookie: std::collections::HashMap<_, _> = elements
        .iter()
        .filter_map(|e| e["cookie"].as_u64().map(|cookie| (cookie, e)))
        .collect();
    if elements.is_empty() || by_cookie.len() != elements.len() {
        return false;
    }
    let mut input = false;
    for element in elements {
        if element_restricted(element, &by_cookie) {
            return false;
        }
        let kind = element["type"].as_u64().unwrap_or(0);
        if !matches!(kind, 1..=4 | 129 | 257 | 513) {
            return false;
        }
        input |= matches!(kind, 1..=4);
        if matches!(kind, 1..=4) {
            let page = element["usagePage"].as_u64().unwrap_or(0);
            let usage = element["usage"].as_u64().unwrap_or(0);
            if !matches!((page,usage),(1,0x30..=0x49 | 0x90..=0x93) | (2,0xb0..=0xc8) | (9,1..=65535))
            {
                return false;
            }
        }
        let mut current = element;
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(current["cookie"].as_u64()) || seen.len() > 64 {
                return false;
            }
            match current["parentCookie"].as_u64() {
                Some(parent) => match by_cookie.get(&parent) {
                    Some(e) => current = e,
                    None => return false,
                },
                None => {
                    if current["type"].as_u64() != Some(513)
                        || current["collectionType"].as_u64() != Some(1)
                        || current["usagePage"].as_u64() != Some(1)
                        || !matches!(current["usage"].as_u64(), Some(4 | 5 | 8))
                    {
                        return false;
                    }
                    break;
                }
            }
        }
    }
    input
}

#[cfg(test)]
mod fallback_tests;
#[cfg(test)]
mod tests;
