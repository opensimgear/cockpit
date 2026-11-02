//! Pure, bounded planner for shared X-Plane RREF subscriptions.
//!
//! Each reader owns a lease containing its selected dataref paths. Wire
//! subscriptions are the union of those leases and the constructor supplied
//! permanent guard paths. Indices are monotonic and are never recycled.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub(crate) const MAX_SELECTED_PATHS: usize = 1024;
pub(crate) const MAX_LEASES: usize = 64;
pub(crate) const MAX_PATH_BYTES: usize = 399;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WireSubscription {
    pub(crate) path: String,
    pub(crate) index: i32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LeaseDelta {
    /// Paths newly assigned active wire indices, ordered lexically by path.
    pub(crate) added: Vec<WireSubscription>,
    /// Paths whose previous active wire indices were retired, ordered lexically by path.
    pub(crate) removed: Vec<WireSubscription>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PlannerError {
    InvalidLeaseId,
    LeaseExists(String),
    LeaseMissing(String),
    TooManyLeases,
    TooManySelectedPaths,
    InvalidPath(String),
    IndexExhausted,
}

impl fmt::Display for PlannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLeaseId => write!(f, "Lease id must be nonempty and contain no NUL."),
            Self::LeaseExists(id) => write!(f, "Lease {id:?} already exists."),
            Self::LeaseMissing(id) => write!(f, "Lease {id:?} does not exist."),
            Self::TooManyLeases => write!(f, "At most {MAX_LEASES} leases are supported."),
            Self::TooManySelectedPaths => write!(
                f,
                "At most {MAX_SELECTED_PATHS} selected paths are supported."
            ),
            Self::InvalidPath(path) => write!(f, "Invalid X-Plane dataref path {path:?}."),
            Self::IndexExhausted => write!(f, "X-Plane RREF wire index space is exhausted."),
        }
    }
}

impl std::error::Error for PlannerError {}

#[derive(Clone, Debug)]
pub(crate) struct SubscriptionPlanner {
    guards: BTreeSet<String>,
    guard_indices: BTreeMap<String, i32>,
    leases: BTreeMap<String, BTreeSet<String>>,
    /// Active selected paths only. Guards are separately retained forever.
    selected: BTreeMap<String, i32>,
    /// Index lookup includes guards and selected paths, including retired
    /// entries being absent. Existing retired indices can never be reissued.
    by_index: BTreeMap<i32, String>,
    next_index: i64,
}

impl SubscriptionPlanner {
    pub(crate) fn new<I, S>(guards: I) -> Result<Self, PlannerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_start_index(guards, 1)
    }

    /// Allocate permanent guard indices in supplied order, starting at the
    /// caller's positive session index.
    pub(crate) fn with_start_index<I, S>(guards: I, first_index: i32) -> Result<Self, PlannerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if first_index <= 0 {
            return Err(PlannerError::IndexExhausted);
        }
        let mut planner = Self {
            guards: BTreeSet::new(),
            guard_indices: BTreeMap::new(),
            leases: BTreeMap::new(),
            selected: BTreeMap::new(),
            by_index: BTreeMap::new(),
            next_index: first_index as i64,
        };
        for path in guards {
            let path = validate_path(path.into())?;
            if !planner.guards.insert(path.clone()) {
                continue;
            }
            let index = planner.allocate_index()?;
            planner.by_index.insert(index, path.clone());
            planner.guard_indices.insert(path, index);
        }
        Ok(planner)
    }

    /// Add a reader's lease. No state changes if validation or allocation fails.
    pub(crate) fn add_lease<I, S>(
        &mut self,
        lease_id: impl Into<String>,
        paths: I,
    ) -> Result<LeaseDelta, PlannerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let id = validate_lease_id(lease_id.into())?;
        if self.leases.contains_key(&id) {
            return Err(PlannerError::LeaseExists(id));
        }
        if self.leases.len() >= MAX_LEASES {
            return Err(PlannerError::TooManyLeases);
        }
        self.apply_lease_change(id, validate_paths(paths)?)
    }

    /// Replace one reader's lease while retaining indices for paths that stay
    /// in the union. The replacement is atomic on every failure.
    pub(crate) fn replace_lease<I, S>(
        &mut self,
        lease_id: &str,
        paths: I,
    ) -> Result<LeaseDelta, PlannerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if !self.leases.contains_key(lease_id) {
            return Err(PlannerError::LeaseMissing(lease_id.to_owned()));
        }
        self.apply_lease_change(lease_id.to_owned(), validate_paths(paths)?)
    }

    /// Remove a reader's lease. Paths retained by another reader remain active
    /// with the same indices.
    pub(crate) fn remove_lease(&mut self, lease_id: &str) -> Result<LeaseDelta, PlannerError> {
        let old = self
            .leases
            .get(lease_id)
            .ok_or_else(|| PlannerError::LeaseMissing(lease_id.to_owned()))?;
        let old = old.clone();
        let mut staged = self.clone();
        staged.leases.remove(lease_id);
        let remaining = staged.union_paths();
        let mut delta = LeaseDelta::default();
        for path in old.difference(&remaining) {
            if let Some(index) = staged.selected.remove(path) {
                staged.by_index.remove(&index);
                delta.removed.push(WireSubscription {
                    path: path.clone(),
                    index,
                });
            }
        }
        *self = staged;
        Ok(delta)
    }

    /// Retire every active wire index and allocate fresh indices for all
    /// permanent guards and selected paths. This is an atomic context fence:
    /// if the remaining positive i32 space cannot hold the whole active set,
    /// the planner remains unchanged.
    pub(crate) fn rotate(&mut self) -> Result<LeaseDelta, PlannerError> {
        let mut staged = self.clone();
        let mut delta = LeaseDelta::default();
        for current in staged.current() {
            delta.removed.push(current);
        }
        staged.guard_indices.clear();
        staged.selected.clear();
        staged.by_index.clear();
        for path in staged.guards.clone() {
            let index = staged.allocate_index()?;
            staged.guard_indices.insert(path.clone(), index);
            staged.by_index.insert(index, path.clone());
            delta.added.push(WireSubscription { path, index });
        }
        for path in staged.union_paths() {
            let index = staged.allocate_index()?;
            staged.selected.insert(path.clone(), index);
            staged.by_index.insert(index, path.clone());
            delta.added.push(WireSubscription { path, index });
        }
        *self = staged;
        Ok(delta)
    }

    /// Current selected subscriptions in stable lexical path order.
    pub(crate) fn selected(&self) -> impl Iterator<Item = WireSubscription> + '_ {
        self.selected.iter().map(|(path, index)| WireSubscription {
            path: path.clone(),
            index: *index,
        })
    }

    /// All active subscriptions, guards first, then selected paths, each in
    /// stable lexical order. Guard indices remain fixed for the planner life.
    pub(crate) fn current(&self) -> impl Iterator<Item = WireSubscription> + '_ {
        self.guards
            .iter()
            .map(|path| WireSubscription {
                path: path.clone(),
                index: self.index_for(path).expect("guard index is permanent"),
            })
            .chain(self.selected())
    }

    /// Resolve a currently active wire index to its path.
    pub(crate) fn path_for_index(&self, index: i32) -> Option<&str> {
        self.by_index
            .get(&index)
            .filter(|path| {
                self.guard_indices.get(*path) == Some(&index)
                    || self.selected.get(*path) == Some(&index)
            })
            .map(String::as_str)
    }

    /// Resolve an active guard or selected path to its stable wire index.
    pub(crate) fn index_for(&self, path: &str) -> Option<i32> {
        self.selected
            .get(path)
            .copied()
            .or_else(|| self.guard_indices.get(path).copied())
    }

    #[cfg(test)]
    fn set_next_index_for_test(&mut self, next: i64) {
        self.next_index = next;
    }

    fn apply_lease_change(
        &mut self,
        id: String,
        paths: BTreeSet<String>,
    ) -> Result<LeaseDelta, PlannerError> {
        let mut staged = self.clone();
        let before = staged.union_paths();
        staged.leases.insert(id, paths);
        let after = staged.union_paths();
        if after.len() > MAX_SELECTED_PATHS {
            return Err(PlannerError::TooManySelectedPaths);
        }
        let mut delta = LeaseDelta::default();
        for path in before.difference(&after) {
            if let Some(index) = staged.selected.remove(path) {
                staged.by_index.remove(&index);
                delta.removed.push(WireSubscription {
                    path: path.clone(),
                    index,
                });
            }
        }
        for path in after.difference(&before) {
            let index = staged.allocate_index()?;
            staged.selected.insert(path.clone(), index);
            staged.by_index.insert(index, path.clone());
            delta.added.push(WireSubscription {
                path: path.clone(),
                index,
            });
        }
        *self = staged;
        Ok(delta)
    }

    fn allocate_index(&mut self) -> Result<i32, PlannerError> {
        if self.next_index > i32::MAX as i64 {
            return Err(PlannerError::IndexExhausted);
        }
        let index = i32::try_from(self.next_index).map_err(|_| PlannerError::IndexExhausted)?;
        self.next_index += 1;
        Ok(index)
    }

    fn union_paths(&self) -> BTreeSet<String> {
        self.leases
            .values()
            .flat_map(|set| set.iter().cloned())
            .filter(|path| !self.guards.contains(path))
            .collect()
    }
}

fn validate_lease_id(id: String) -> Result<String, PlannerError> {
    if id.is_empty() || id.contains('\0') {
        Err(PlannerError::InvalidLeaseId)
    } else {
        Ok(id)
    }
}

fn validate_paths<I, S>(paths: I) -> Result<BTreeSet<String>, PlannerError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    paths
        .into_iter()
        .map(Into::into)
        .map(validate_path)
        .collect()
}

fn validate_path(path: String) -> Result<String, PlannerError> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        Err(PlannerError::InvalidPath(path))
    } else {
        Ok(path)
    }
}

#[cfg(test)]
#[path = "subscriptions/tests.rs"]
mod tests;
