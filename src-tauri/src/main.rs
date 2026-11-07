#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // #160 askpass mode: ssh runs this exe as SSH_ASKPASS → print the one-shot password, no app
    if let Ok(p) = std::env::var("CLAURACLE_ASKPASS") { println!("{p}"); return; }
    clauracle_lib::run()
}
