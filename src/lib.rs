use simple_broadcaster::Subscriber;
use std::{
    io::{self, Read, Write},
    process::{Command as StdCommand, Stdio},
    sync::mpsc::{self, channel, Receiver, RecvError, SendError, Sender},
    thread,
};
use thiserror::Error as ThisError;
use tracing::{self, trace};

pub struct Command {
    std_command: StdCommand,
}

impl From<StdCommand> for Command {
    fn from(std_command: StdCommand) -> Self {
        Self { std_command }
    }
}

pub struct StdinSender(Sender<String>);
pub struct StdoutReceiver(Receiver<String>);
pub struct StderrReceiver(Receiver<String>);

impl Command {
    pub fn run(
        &mut self,
        canceller: Subscriber<()>,
    ) -> Result<(StdinSender, StdoutReceiver, StderrReceiver), Error> {
        let cmd = self
            .std_command
            .get_program()
            .to_owned()
            .into_string()
            .unwrap();
        trace!("preparing to run '{cmd:}'");
        let (tx_in, rx_in) = channel::<String>();
        let (tx_out, rx_out) = channel::<String>();
        let (tx_err, rx_err) = channel::<String>();
        let mut pid = self
            .std_command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = pid.stdin.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                trace!("'{cmd:}' is in stdin recv");
                while let Ok(stdin_text) = rx_in.recv() {
                    let stdin_text: String = stdin_text;
                    trace!("'{cmd:}' received '{stdin_text}' in stdin thread");
                    stdin.write_all(stdin_text.as_bytes()).unwrap();
                }
                trace!("exiting the stdin thread of '{cmd:}'");
            });
        }
        if let Some(mut stdout) = pid.stdout.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                let mut buf: [u8; 128] = [0; 128];
                trace!("'{cmd:}' is in stdout read");
                while let Ok(read_bytes) = stdout.read(&mut buf) {
                    if read_bytes == 0 {
                        trace!("'{cmd:}' stdout closed");
                        break;
                    }
                    let stdout_text = String::from_utf8_lossy(&buf[0..read_bytes]);
                    trace!("'{cmd:}' received '{stdout_text}' in stdout thread");
                    if tx_out.send(stdout_text.into_owned()).is_err() {
                        break;
                    }
                }
                trace!("exiting the stdout thread of '{cmd:}'");
            });
        }

        if let Some(mut stderr) = pid.stderr.take() {
            let cmd = cmd.clone();
            thread::spawn(move || {
                let mut buf: [u8; 128] = [0; 128];
                trace!("'{cmd:}' is in stderr read");
                while let Ok(read_bytes) = stderr.read(&mut buf) {
                    if read_bytes == 0 {
                        trace!("'{cmd:}' stderr closed");
                        break;
                    }
                    let stderr_text = String::from_utf8_lossy(&buf[0..read_bytes]);
                    trace!("'{cmd:}' received '{stderr_text}' in stderr thread");
                    if tx_err.send(stderr_text.into_owned()).is_err() {
                        break;
                    }
                }
                trace!("exiting the stderr thread of '{cmd:}'");
            });
        }

        let cmd_ = cmd.clone();
        thread::spawn(move || {
            if let Ok(_) = canceller.recv() {
                let _ = pid.kill();
            }
            trace!("exiting the canceller thread of '{cmd_:}'");
        });

        trace!("exiting run '{cmd:}'");
        Ok((
            StdinSender(tx_in),
            StdoutReceiver(rx_out),
            StderrReceiver(rx_err),
        ))
    }
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    SendError(#[from] mpsc::SendError<String>),
    #[error("thread could not join")]
    ThreadCouldNotJoin(String),
}

impl StdinSender {
    pub fn send(&self, input: String) -> Result<(), SendError<String>> {
        self.0.send(input)
    }
}

impl StdoutReceiver {
    pub fn recv(&self) -> Result<String, RecvError> {
        self.0.recv()
    }
}

impl StderrReceiver {
    pub fn recv(&self) -> Result<String, RecvError> {
        self.0.recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result as AnyResult;
    // use pretty_assertions::assert_eq;
    use simple_broadcaster::broadcasting_channel;
    use std::{thread, time::Duration};

    #[test]
    fn kill_test() -> AnyResult<()> {
        let _ = tracing_subscriber::fmt::try_init();
        let (broadcaster, subscriber) = broadcasting_channel("test the managed command");
        let mut std_cmd = std::process::Command::new("managed-command-test-process");
        std_cmd.env("PATH", "testing");
        let mut cmd = Command::from(std_cmd);
        let (_stdin, _stdout, _stderr) = cmd.run(subscriber)?;
        trace!("will wait for 1 sec");
        thread::sleep(Duration::from_secs(1));
        trace!("will kill the process now. and sleep for 1 more sec");
        broadcaster.broadcast(())?;
        thread::sleep(Duration::from_secs(1));
        Ok(())
    }

    #[test]
    fn check_input_output() -> AnyResult<()> {
        let _ = tracing_subscriber::fmt::try_init();
        let (broadcaster, subscriber) = broadcasting_channel("test the managed command");
        let mut std_cmd = std::process::Command::new("managed-command-test-process");
        std_cmd.env("PATH", "testing");
        let mut cmd = Command::from(std_cmd);
        let (stdin, stdout, _stderr) = cmd.run(subscriber)?;
        let handle = thread::spawn(move || loop {
            match stdout.recv() {
                Ok(out) => trace!("received: '{}'", out.trim()),
                Err(mpsc::RecvError) => break,
            }
        });
        stdin.send("second input\n".to_owned())?;
        stdin.send("first input\n".to_owned())?;
        thread::sleep(Duration::from_secs(2));
        broadcaster.broadcast(())?;
        stdin.send("second input\n".to_owned())?;
        let _ = handle.join();
        Ok(())
    }

    #[test]
    fn check_only_output() -> AnyResult<()> {
        let _ = tracing_subscriber::fmt::try_init();
        let (broadcaster, subscriber) = broadcasting_channel("test the managed command");
        let mut std_cmd = std::process::Command::new("managed-command-test-process");
        std_cmd.env("PATH", "testing");
        let mut cmd = Command::from(std_cmd);
        let (_stdin, stdout, _stderr) = cmd.run(subscriber)?;
        let thread_handle = thread::spawn(move || loop {
            let out = stdout.recv().unwrap();
            trace!("received: '{}'", out.trim());
            //assert_eq!(out.trim(), "this is the default output!");
        });
        thread::sleep(Duration::from_secs(2));
        broadcaster.broadcast(())?;
        let _ = thread_handle.join();
        Ok(())
    }
}
