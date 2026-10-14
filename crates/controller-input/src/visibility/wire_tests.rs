use super::super::policy::tests::state;
use super::*;
struct Fake {
    state: State,
    calls: usize,
    bad: bool,
}
impl Io for Fake {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        self.calls += 1;
        match function {
            2048 => Ok(encode(&self.state.applications)),
            2050 => Ok(encode(&self.state.hidden)),
            2052 => Ok(vec![u8::from(self.state.active)]),
            2054 => Ok(vec![u8::from(self.state.inverse)]),
            2049 => {
                self.state.applications = list(input.unwrap())?;
                Ok(vec![])
            }
            2051 => {
                self.state.hidden = list(input.unwrap())?;
                Ok(vec![])
            }
            2053 => {
                self.state.active = input.unwrap() == [1];
                Ok(if self.bad { vec![1] } else { vec![] })
            }
            2055 => {
                self.state.inverse = input.unwrap() == [1];
                Ok(vec![])
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn abi_reads_and_single_property_writes_are_bounded_verified_and_exact() {
    let mut driver = Driver(Fake {
        state: state(),
        calls: 0,
        bad: false,
    });
    let mut expected = driver.read().unwrap();
    for next in 0..4 {
        let mut desired = expected.clone();
        match next {
            0 => desired.applications = vec![r"\Device\Volume\App.exe".into()],
            1 => desired.hidden = vec![r"HID\DEVICE\1".into()],
            2 => desired.active = true,
            _ => desired.inverse = true,
        };
        driver.change(&expected, &desired).unwrap();
        expected = desired;
    }
    assert_eq!(driver.read().unwrap(), expected);
    assert!(driver.change(&expected, &expected).is_err());
    let mut bad = expected.clone();
    bad.hidden.clear();
    bad.inverse = false;
    assert!(driver.change(&expected, &bad).is_err());
    let mut stale = expected.clone();
    stale.active = false;
    assert!(driver.change(&stale, &expected).is_err());
    driver.0.bad = true;
    let mut changed = expected.clone();
    changed.active = false;
    assert!(driver.change(&expected, &changed).is_err());
}
#[test]
fn multi_strings_reject_truncation_trailing_data_and_invalid_unicode() {
    for data in [
        vec![],
        vec![0],
        vec![1, 0],
        vec![1, 0, 0, 0],
        vec![0, 0, 1, 0, 0, 0, 0, 0],
        vec![0, 0xd8, 0, 0, 0, 0],
        vec![0; LIMIT + 2],
    ] {
        assert!(list(&data).is_err(), "{data:?}");
    }
    assert!(list(&[0, 0]).unwrap().is_empty());
    assert!(list(&[0, 0, 0, 0]).unwrap().is_empty());
    let values = vec!["one".into(), "äπ".into()];
    assert_eq!(list(&encode(&values)).unwrap(), values);
}
#[test]
fn inconsistent_read_boolean_and_mutation_readback_are_rejected() {
    struct Broken {
        state: State,
        calls: usize,
        mode: u8,
    }
    impl Io for Broken {
        fn call(&mut self, f: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
            self.calls += 1;
            if self.mode == 0 && f == 2052 {
                return Ok(vec![2]);
            }
            if self.mode == 1 && self.calls == 8 {
                return Ok(vec![1]);
            }
            if input.is_some() {
                return Ok(vec![]);
            }
            Fake {
                state: self.state.clone(),
                calls: 0,
                bad: false,
            }
            .call(f, None)
        }
    }
    for mode in [0, 1] {
        assert!(Driver(Broken {
            state: state(),
            calls: 0,
            mode
        })
        .read()
        .is_err());
    }
    let mut driver = Driver(Broken {
        state: state(),
        calls: 0,
        mode: 2,
    });
    let mut desired = state();
    desired.active = true;
    assert!(driver.change(&state(), &desired).is_err());
}
