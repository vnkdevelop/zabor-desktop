use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) const ROOT: &str = "ZABOR_HIDDEN_DURABILITY_TEST_ROOT";
pub(super) const STAGE: &str = "ZABOR_HIDDEN_DURABILITY_TEST_STAGE";
pub(super) const ACCEPT: &str = "ZABOR_HIDDEN_DURABILITY_TEST_ACCEPT";

pub(super) struct Process {
    child: Option<Child>,
    messages: Receiver<String>,
    reader: Option<JoinHandle<()>>,
}

impl Process {
    pub(super) fn start(test: &str, root: &Path, stage: &str, accept: bool) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(ROOT, root)
            .env(STAGE, stage)
            .env(ACCEPT, if accept { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let (sender, messages) = mpsc::channel();
        let mut process = Self {
            child: Some(child),
            messages,
            reader: None,
        };
        let output = process.child.as_mut().unwrap().stdout.take().unwrap();
        process.reader = Some(
            std::thread::Builder::new()
                .name("hidden-durability-process-receipt".into())
                .spawn(move || {
                    for line in BufReader::new(output).lines() {
                        let Ok(line) = line else { break };
                        if line.contains("HIDDEN-CHECKPOINT ") && sender.send(line).is_err() {
                            break;
                        }
                    }
                })
                .unwrap(),
        );
        process
    }

    pub(super) fn checkpoint(&self, stage: &str) {
        let observed = self.messages.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(observed.contains(&format!("HIDDEN-CHECKPOINT {stage}")));
    }

    pub(super) fn terminate(mut self) -> ExitStatus {
        let child = self.child.as_mut().unwrap();
        let pid = child.id();
        child.kill().unwrap();
        let status = child.wait().unwrap();
        drop(self.child.take());
        self.reader.take().unwrap().join().unwrap();
        assert!(!status.success());
        println!("hidden durability process pid={pid} terminate_observed=true exit_observed=true receipt_thread_join=true physical_power_loss=false");
        status
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn checkpoint(stage: &str) {
    println!("HIDDEN-CHECKPOINT {stage}");
    std::io::stdout().flush().unwrap();
    let mut release = String::new();
    std::io::stdin().read_line(&mut release).unwrap();
}
