use std::{
    env,
    io::{self, Write},
    process::ExitCode,
    thread,
};

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["version"] {
        println!("Android Debug Bridge version 1.0.41");
        return ExitCode::SUCCESS;
    }
    if args != ["devices", "-l"] {
        eprintln!("Expected exactly: devices -l");
        return ExitCode::FAILURE;
    }

    match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
        Ok("empty") => println!("List of devices attached\n"),
        Ok("devices") => {
            eprintln!("* daemon started successfully");
            println!(
                "List of devices attached\nemulator-5554 device product:sdk model:Pixel_8 device:emu transport_id:1\nphone unauthorized\nother offline\n"
            );
        }
        Ok("failure") => {
            eprintln!("ADB fixture cannot contact server");
            return ExitCode::FAILURE;
        }
        Ok("timeout") => loop {
            thread::park();
        },
        Ok("malformed") => println!("Not an ADB device listing"),
        Ok("invalid_utf8") => {
            io::stdout().write_all(&[0xff, 0xfe]).unwrap();
        }
        _ => return ExitCode::FAILURE,
    }
    ExitCode::SUCCESS
}
