//! Pure device selection: exact physical groups, no host permission changes.
use super::{launch::Request, session::Source};
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub path: String,
    pub group: String,
    pub identity: String,
    pub virtual_device: bool,
    pub device: u64,
    pub inode: u64,
    pub filesystem: u64,
}
pub(crate) fn device_path(path: &str) -> bool {
    ["/dev/input/event", "/dev/input/js", "/dev/hidraw"]
        .iter()
        .any(|prefix| {
            path.strip_prefix(prefix).is_some_and(|n| {
                !n.is_empty() && n.len() <= 8 && n.bytes().all(|c| c.is_ascii_digit())
            })
        })
}
pub(crate) fn selection(nodes: &[Node], sources: &[Source]) -> Result<Vec<Node>, String> {
    if nodes.len() > 256 {
        return Err("Too many input nodes for a bounded isolated view.".into());
    }
    super::session::validate_sources(sources, sources, false)?;
    let mut paths = BTreeSet::new();
    for node in nodes {
        if !device_path(&node.path) || node.group.is_empty() || !paths.insert(&node.path) {
            return Err("Invalid or ambiguous input-node inventory.".into());
        }
    }
    let mut hidden = BTreeSet::new();
    for source in sources {
        let matching: Vec<_> = nodes
            .iter()
            .filter(|n| {
                !n.identity.is_empty() && n.identity == source.interface && !n.virtual_device
            })
            .collect();
        if matching.len() != 1 {
            return Err(
                "Selected capture identity cannot be resolved to exactly one native event node."
                    .into(),
            );
        }
        hidden.insert(matching[0].group.clone());
    }
    if nodes
        .iter()
        .any(|n| n.virtual_device && hidden.contains(&n.group))
    {
        return Err("Physical and virtual device groups overlap.".into());
    }
    Ok(nodes
        .iter()
        .filter(|n| !hidden.contains(&n.group))
        .cloned()
        .collect())
}
pub(crate) fn arguments(
    request: &Request,
    executable: &str,
    bundle: &str,
    nodes: &[(String, i32)],
    status_fd: i32,
    filter_fd: i32,
    release_fd: i32,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "--unshare-all",
        "--unshare-user",
        "--disable-userns",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--clearenv",
        "--ro-bind",
        "/usr",
        "/usr",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--dir",
        "/dev/input",
        "--tmpfs",
        "/tmp",
        "--dir",
        "/home",
        "--dir",
        "/home/player",
        "--dir",
        "/run",
        "--dir",
        "/sys",
        "--setenv",
        "HOME",
        "/home/player",
        "--setenv",
        "PATH",
        "/usr/bin:/bin",
        "--setenv",
        "LANG",
        "C.UTF-8",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    args.extend([
        "--ro-bind".into(),
        bundle.into(),
        "/app".into(),
        "--chdir".into(),
        "/app".into(),
        "--json-status-fd".into(),
        status_fd.to_string(),
        "--seccomp".into(),
        filter_fd.to_string(),
        "--block-fd".into(),
        release_fd.to_string(),
    ]);
    for (path, fd) in nodes {
        args.extend([
            "--dev-bind".into(),
            format!("/proc/self/fd/{fd}"),
            path.clone(),
        ]);
    }
    args.extend([
        "--remount-ro".into(),
        "/dev".into(),
        "--".into(),
        executable.into(),
    ]);
    args.extend(request.arguments.clone());
    args
}

#[cfg(test)]
#[path = "launch_plan_tests.rs"]
mod tests;
