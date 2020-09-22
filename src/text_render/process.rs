use std::io::Read;
use std::io::Write;
use std::process::Command;
pub struct ProcessManager {
    stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
}
impl ProcessManager {
    pub fn new() -> ProcessManager {
        let mut p = Command::new("powershell")
            .spawn()
            .expect("failed to launch powershell");
         ProcessManager {
            stdin: p.stdin.unwrap(),
            stdout: p.stdout.unwrap(),
        }
    }
    fn write(&mut self, to_write: String) {
        self.stdin.write(to_write.as_bytes());
    }
    fn read(&mut self) -> String {
        let mut s = String::new();
        self.stdout.read_to_string(&mut s);
        return s;
    }
}
