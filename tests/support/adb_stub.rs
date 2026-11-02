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
    if args.len() == 4 && args[0] == "-s" && args[2..] == ["shell", "getprop"] {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("info") => print!("{}", include_str!("../fixtures/getprop/synthetic.txt")),
            Ok("info_tc57") => print!("{}", include_str!("../fixtures/getprop/tc57.txt")),
            Ok("info_emulator") => print!("{}", include_str!("../fixtures/getprop/emulator.txt")),
            Ok("info_partial") => println!("[ro.product.model]: [Partial Phone]\n"),
            Ok("info_unauthorized") | Ok("info_offline") | Ok("info_not_found") => {
                eprintln!(
                    "device unavailable: {}",
                    env::var("ANDROID_MCP_TEST_ADB_MODE").unwrap()
                );
                return ExitCode::FAILURE;
            }
            Ok("info_timeout") => loop {
                thread::park();
            },
            Ok("info_malformed") => println!("not property output"),
            Ok("info_invalid_utf8") => {
                io::stdout().write_all(&[0xff, 0xfe]).unwrap();
            }
            _ => return ExitCode::FAILURE,
        }
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
