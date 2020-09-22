use std::io::Read;
use std::io::Write;
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
pub struct ProcessManager {
    stdin: std::process::ChildStdin,

    stdout_reciever: Receiver<String>,
}
impl ProcessManager {
    pub fn new() -> ProcessManager {
        let mut p = Command::new("powershell")
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .expect("failed to launch powershell");
        let (stdout_sender, stdout_reciever) = channel();
        let mut stdout = p.stdout.unwrap();
        std::thread::spawn(move || {
            read(&mut stdout, stdout_sender);
        });
        ProcessManager {
            stdin: p.stdin.unwrap(),
            stdout_reciever,
        }
    }
    pub fn write(&mut self, to_write: String) {
        self.stdin.write(to_write.as_bytes());
    }
    pub fn read(&mut self) -> String {
        self.stdout_reciever
            .try_iter()
            .fold(String::new(), |sum, s| sum + &s)
    }
}
fn read(std_out: &mut ChildStdout, send: Sender<String>) {
    const BUFFER_SIZE: usize = 10;
    loop {
        let mut buff = [0; BUFFER_SIZE];
        std_out.read_exact(&mut buff);
        let s = unsafe { String::from_utf8_unchecked(buff.to_vec()) };
        send.send(s);
    }
}
