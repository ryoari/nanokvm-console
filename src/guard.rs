use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub const PINS: &str = include_str!(concat!(env!("OUT_DIR"), "/verified.json"));
pub fn path() -> PathBuf {
    let executable = std::env::current_exe().unwrap_or_default();
    let mut parent = executable
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    if cfg!(test) && parent.file_name().is_some_and(|s| s == "deps") {
        parent.pop();
    }
    parent.join("console_guard")
}
pub fn ready() -> bool {
    std::fs::read(path())
        .is_ok_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == env!("GUARD_SHA256"))
}
pub struct Guard {
    failed: bool,
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Guard {
    pub fn start() -> Result<Self> {
        if !ready() {
            return Err(
                "Matching input guard unavailable. Reinstall the console and its guard together.",
            );
        }
        let mut child = Command::new(path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "Cannot start Lean input guard.")?;
        let input = child.stdin.take().ok_or("Lean input unavailable.")?;
        let output = BufReader::new(child.stdout.take().ok_or("Lean output unavailable.")?);
        Ok(Self {
            failed: false,
            child,
            input,
            output,
        })
    }
    pub async fn event(&mut self, event: Value) -> Result<Value> {
        if self.failed {
            return Err("Input guard failed. Release is unconfirmed.");
        }
        let text = serde_json::to_string(&event).map_err(|_| "Invalid input event.")?;
        if text.len() > 4096 {
            return Err("Input event too large.");
        }
        let result = timeout(Duration::from_secs(2), async {
            self.input
                .write_all(format!("{text}\n").as_bytes())
                .await
                .map_err(|_| "Lean input failed.")?;
            let mut line = String::new();
            self.output
                .read_line(&mut line)
                .await
                .map_err(|_| "Lean output failed.")?;
            if line.len() > 8192 {
                return Err("Lean response too large.");
            }
            serde_json::from_str(&line).map_err(|_| "Lean response invalid.")
        })
        .await
        .map_err(|_| "Lean response timed out.")
        .and_then(|value| value);
        if result.is_err() {
            self.failed = true;
            let _ = self.child.start_kill();
        }
        result
    }
    pub async fn stop(mut self) {
        let _ = self.input.shutdown().await;
        if timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}
pub fn browser_event(value: &Value) -> Result<Value> {
    let event = value.get("value").ok_or("Missing input event.")?;
    if !matches!(
        event.get("op").and_then(Value::as_str),
        Some("arm" | "disarm" | "keyboard" | "pointer")
    ) {
        return Err("Unsupported input operation.");
    }
    Ok(event.clone())
}
pub fn frames(value: &Value) -> Result<Vec<Vec<u8>>> {
    serde_json::from_value(value.get("frames").cloned().unwrap_or(json!(null)))
        .map_err(|_| "Invalid Lean frames.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_lean_requires_arm_validates_bounds_and_releases() {
        let mut g = Guard::start().unwrap();
        assert!(
            !g.event(json!({"op":"arm"})).await.unwrap()["accepted"]
                .as_bool()
                .unwrap()
        );
        g.event(json!({"op":"connected"})).await.unwrap();
        assert!(
            frames(
                &g.event(json!({"op":"keyboard","modifier":0,"keys":[4]}))
                    .await
                    .unwrap()
            )
            .unwrap()
            .is_empty()
        );
        g.event(json!({"op":"arm"})).await.unwrap();
        assert!(
            frames(
                &g.event(json!({"op":"keyboard","modifier":256,"keys":[4]}))
                    .await
                    .unwrap()
            )
            .unwrap()
            .is_empty()
        );
        let input = g
            .event(json!({"op":"keyboard","modifier":2,"keys":[4]}))
            .await
            .unwrap();
        assert_eq!(frames(&input).unwrap().len(), 1);
        let closed = g.event(json!({"op":"close"})).await.unwrap();
        assert_eq!(closed["armed"], false);
        assert!(!frames(&closed).unwrap().is_empty());
        let reopened = g.event(json!({"op":"connected"})).await.unwrap();
        assert_eq!(reopened["armed"], false);
        g.stop().await;
    }
    #[tokio::test]
    async fn malformed_input_disarms_and_releases() {
        let mut g = Guard::start().unwrap();
        g.event(json!({"op":"connected"})).await.unwrap();
        g.event(json!({"op":"arm"})).await.unwrap();
        g.event(json!({"op":"keyboard","modifier":0,"keys":[4]}))
            .await
            .unwrap();
        g.event(json!({"op":"pointer","buttons":1,"x":100,"y":200,"wheel":0}))
            .await
            .unwrap();
        let rejected = g
            .event(json!({"op":"keyboard","modifier":-1,"keys":[]}))
            .await
            .unwrap();
        assert_eq!(rejected["armed"], false);
        assert_eq!(rejected["accepted"], false);
        let reports = frames(&rejected).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(&reports[0][13..], &[0; 8]);
        assert_eq!(&reports[1][13..], &[0, 100, 0, 200, 0, 0, 0]);
        assert!(
            frames(
                &g.event(json!({"op":"keyboard","modifier":0,"keys":[4]}))
                    .await
                    .unwrap()
            )
            .unwrap()
            .is_empty()
        );
        g.stop().await;
    }
    #[tokio::test]
    async fn timed_out_guard_is_never_reused_for_release() {
        let mut child=Command::new("sh").arg("-c").arg("read line; sleep 2.3; printf '%s\\n' '{\"accepted\":true,\"armed\":true,\"frames\":[[99]]}'; read line").stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut g = Guard {
            failed: false,
            child,
            input,
            output,
        };
        assert!(
            g.event(json!({"op":"keyboard","modifier":0,"keys":[4]}))
                .await
                .is_err()
        );
        let start = std::time::Instant::now();
        assert!(g.event(json!({"op":"close"})).await.is_err());
        assert!(start.elapsed() < Duration::from_millis(200));
        g.stop().await;
    }
    #[test]
    fn browser_cannot_forge_internal_transitions() {
        for op in ["connected", "close", "raw", "reset"] {
            assert!(browser_event(&json!({"value":{"op":op}})).is_err());
        }
    }
}
