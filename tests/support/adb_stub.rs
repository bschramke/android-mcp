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
    if args.len() == 8
        && args[0] == "-s"
        && args[2..5] == ["logcat", "-d", "-t"]
        && args[6..] == ["-v", "threadtime"]
    {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected logcat serial argument");
            return ExitCode::FAILURE;
        }
        let mode = env::var("ANDROID_MCP_TEST_ADB_MODE").unwrap_or_default();
        let valid_lines = match mode.as_str() {
            "logcat_explicit" => args[5] == "17",
            "logcat_bounds" => args[5] == "1" || args[5] == "2000",
            _ => args[5] == "200",
        };
        if !valid_lines {
            eprintln!("Unexpected logcat line limit");
            return ExitCode::FAILURE;
        }
        match mode.as_str() {
            "logcat" | "logcat_explicit" | "logcat_bounds" => {
                eprintln!("Synthetic ADB diagnostic, not log output");
                print!("{}", include_str!("../fixtures/logcat/threadtime.txt"));
            }
            "logcat_empty" => {}
            "logcat_failure" => {
                eprintln!("device offline");
                return ExitCode::FAILURE;
            }
            "logcat_timeout" => loop {
                thread::park();
            },
            "logcat_large" => {
                let _ = io::stdout().write_all(&vec![b'x'; 256 * 1024 + 4096]);
                loop {
                    thread::park();
                }
            }
            "logcat_stderr_large" => {
                let _ = io::stderr().write_all(&vec![b'e'; 16 * 1024 + 4096]);
                loop {
                    thread::park();
                }
            }
            "logcat_invalid_utf8" => {
                io::stdout().write_all(&[0xff, 0xfe]).unwrap();
            }
            _ => return ExitCode::FAILURE,
        }
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
