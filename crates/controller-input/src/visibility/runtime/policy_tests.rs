use super::*;
use crate::visibility::windows::PolicyWatch;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_DWORD,
};

struct RegistryTree(String);

impl RegistryTree {
    fn new() -> Self {
        let tree = Self(format!(r"Software\OpenSimGear-Visibility-{}", uuid::Uuid::new_v4()));
        drop(tree.parameters());
        tree
    }

    fn parameters(&self) -> TestKey {
        let path: Vec<_> = format!(r"{}\Parameters", self.0).encode_utf16().chain([0]).collect();
        let mut handle = null_mut();
        assert_eq!(unsafe {
            RegCreateKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, null(), 0,
                KEY_SET_VALUE, null(), &mut handle, null_mut())
        }, 0);
        TestKey(handle)
    }

    fn remove(&self) -> u32 {
        let path: Vec<_> = self.0.encode_utf16().chain([0]).collect();
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) }
    }
}

impl Drop for RegistryTree {
    fn drop(&mut self) { self.remove(); }
}

struct TestKey(HKEY);

impl TestKey {
    fn set(&self, value: u32) {
        let name: Vec<_> = "Probe".encode_utf16().chain([0]).collect();
        let bytes = value.to_le_bytes();
        assert_eq!(unsafe {
            RegSetValueExW(self.0, name.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), bytes.len() as u32)
        }, 0);
    }
}

impl Drop for TestKey {
    fn drop(&mut self) { unsafe { RegCloseKey(self.0) }; }
}

struct PolicyProbe {
    driver: Box<dyn Adapter>,
    key: String,
    reads: Arc<AtomicU64>,
}

impl Backend for PolicyProbe {
    fn read(&mut self) -> Result<State, String> {
        let result = self.driver.read();
        self.reads.fetch_add(1, Ordering::Release);
        result
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        self.driver.change(expected, desired)
    }
}

impl Adapter for PolicyProbe {
    fn executable(&mut self, path: &str) -> Result<String, String> {
        self.driver.executable(path)
    }
    fn instance(&mut self, interface: &str) -> Result<String, String> {
        self.driver.instance(interface)
    }
    fn watch_policy(&mut self, wake: Arc<NativeWake>) -> Option<PolicyWatch> {
        Some(PolicyWatch::start(HKEY_CURRENT_USER, &self.key, wake).unwrap())
    }
}

#[test]
fn registry_subtree_changes_replace_policy_polling_until_the_watch_is_lost() {
    let tree = RegistryTree::new();
    let key = tree.0.clone();
    let ReadyCapture { registry, task, factory, capture, state, fresh_read, resume_read } = ReadyCapture::new();
    let reads = Arc::new(AtomicU64::new(0));
    let captured_reads = reads.clone();
    let factory: Factory = Arc::new(move || Ok(Box::new(PolicyProbe {
        driver: factory()?, key: key.clone(), reads: captured_reads.clone(),
    })));
    let temp = crate::test_directory();
    let path = temp.path().join("visibility.json");
    let mut worker = Worker::default();
    let Task::Apply { ids, .. } = &task else { unreachable!() };
    let id = ids[0].clone();
    let status = worker.start(task, path.clone(), factory, capture).unwrap();
    fresh_read.recv_timeout(Duration::from_secs(4)).unwrap();
    registry.lock().unwrap().report(&id, 1, &BTreeMap::from([(0, 0), (1, 0)])).unwrap();
    resume_read.send(()).unwrap();
    assert!(worker.completion().wait(&status.job_id, false, Duration::from_secs(1)).unwrap());
    assert_eq!(worker.snapshot().unwrap().phase, "active");

    let before = reads.load(Ordering::Acquire);
    std::thread::sleep(DRIVER_AUDIT + Duration::from_millis(50));
    assert_eq!(reads.load(Ordering::Acquire), before);

    let wait_reads = |expected| {
        let deadline = Instant::now() + Duration::from_secs(3);
        while reads.load(Ordering::Acquire) < expected {
            assert!(Instant::now() < deadline, "expected policy read {expected}");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let parameters = tree.parameters();
    parameters.set(1);
    wait_reads(before + 1);
    parameters.set(2);
    wait_reads(before + 2);
    drop(parameters);
    assert_eq!(tree.remove(), 0);
    wait_reads(before + 4);

    worker.stop(Some(&status.job_id)).unwrap();
    assert!(worker.completion().wait(&status.job_id, true, Duration::from_secs(1)).unwrap());
    assert_eq!(worker.snapshot().unwrap().phase, "restored");
    assert!(!state.lock().unwrap().active);
    assert!(journal::inspect(&path).unwrap().unwrap().clean);
}
