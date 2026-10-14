use super::*;
use osg_mapping::engine::{Counter, RunToken};
fn python() -> std::path::PathBuf {
    std::env::var_os("OSG_TEST_PYTHON")
        .map(Into::into)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        })
}
fn spawn(code: &str) -> Result<Worker, String> {
    Worker::spawn(
        &python(),
        &["-u".into(), "-c".into(), code.into()],
        "fixture",
        Duration::from_millis(500),
    )
}
fn message() -> Message {
    Message::Stop {
        token: RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        },
    }
}
#[test]
fn actual_process_checks_handshake_order_eof_oversize_and_deadline() {
    let hello = "print('{\"ok\":true,\"backend\":\"fixture\",\"protocol\":1}',flush=True)\n";
    let mut child = spawn(&format!(
        "{hello}import sys,json\nfor line in sys.stdin:\n print(json.dumps({{'ok':True,'token':json.loads(line)['token']}}),flush=True)\n"
    ))
    .unwrap();
    assert!(child.alive());
    child.request(&message()).unwrap();
    child.request(&message()).unwrap();
    child.terminate();
    assert!(!child.alive());
    assert!(child.request(&message()).is_err());
    for code in ["print('{}')".to_owned(),"print('x'*70000)".into(),"print('broken')".into(),
        format!("{hello}import sys\nfor line in sys.stdin:\n print('{{\"ok\":false,\"error\":\"fixture failure\"}}',flush=True)\n"),
        format!("{hello}import time\ntime.sleep(2)\n")] {
        match spawn(&code) {
            Err(_)=>{},
            Ok(mut child)=>{assert!(child.request(&message()).is_err());assert!(!child.alive());}
        }
    }
    assert!(Worker::spawn(
        Path::new("missing-osg-worker"),
        &[],
        "fixture",
        Duration::from_millis(1)
    )
    .is_err());
    let mut child = spawn(&format!("{hello}import time\ntime.sleep(2)\n")).unwrap();
    let huge = Message::Start {
        token: RunToken {
            session: "x".repeat(FRAME_LIMIT),
            generation: Counter(1),
        },
        bindings: vec![],
    };
    assert!(child.request(&huge).is_err());
}
