use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    process::ExitCode,
    thread,
};

const SCREENSHOT_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3, 2, 0,
    239, 191, 175, 219, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const SCREEN_RECORDING_MP4: &[u8] = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2";
fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["version"] {
        println!("Android Debug Bridge version 1.0.41");
        return ExitCode::SUCCESS;
    }
    if args.len() == 8
        && args[0] == "-s"
        && args[2..5] == ["shell", "-tt", "screenrecord"]
        && args[5..7] == ["--time-limit", "180"]
    {
        if args[1] != "emulator-5554" {
            eprintln!("Unexpected screen-recording serial argument");
            return ExitCode::FAILURE;
        }
        if !args[7].starts_with("/sdcard/.android-mcp-recording-") || !args[7].ends_with(".mp4") {
            eprintln!("Unexpected remote screen-recording path");
            return ExitCode::FAILURE;
        }
        let trace = env::var_os("ANDROID_MCP_TEST_RECORDING_TRACE").unwrap();
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trace)
                .unwrap(),
            "start {}",
            args[7]
        )
        .unwrap();
        println!("screenrecord started");
        eprintln!("screenrecord diagnostic");
        let mut byte = [0];
        loop {
            match io::stdin().read(&mut byte) {
                Ok(0) => break,
                Ok(_) if byte[0] == 3 => break,
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Cannot read screen-recording control input: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        writeln!(
            OpenOptions::new().append(true).open(&trace).unwrap(),
            "stopped"
        )
        .unwrap();
        return ExitCode::SUCCESS;
    }
    if args.len() == 5 && args[0] == "-s" && args[2] == "pull" {
        if args[1] != "emulator-5554" || !args[3].starts_with("/sdcard/.android-mcp-recording-") {
            eprintln!("Unexpected screen-recording pull arguments");
            return ExitCode::FAILURE;
        }
        std::fs::write(&args[4], SCREEN_RECORDING_MP4).unwrap();
        let trace = env::var_os("ANDROID_MCP_TEST_RECORDING_TRACE").unwrap();
        writeln!(
            OpenOptions::new().append(true).open(&trace).unwrap(),
            "pulled"
        )
        .unwrap();
        return ExitCode::SUCCESS;
    }
    if args.len() == 6 && args[0] == "-s" && args[2..5] == ["shell", "rm", "-f"] {
        if args[1] != "emulator-5554" || !args[5].starts_with("/sdcard/.android-mcp-recording-") {
            eprintln!("Unexpected screen-recording cleanup arguments");
            return ExitCode::FAILURE;
        }
        let trace = env::var_os("ANDROID_MCP_TEST_RECORDING_TRACE").unwrap();
        writeln!(
            OpenOptions::new().append(true).open(&trace).unwrap(),
            "cleaned"
        )
        .unwrap();
        return ExitCode::SUCCESS;
    }
    if args.len() == 5 && args[0] == "-s" && args[2..] == ["shell", "id", "-u"] {
        if args[1] != "emulator-5554" {
            eprintln!("Unexpected health-check serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("health_online") => println!("2000"),
            Ok("health_root") => println!("0"),
            Ok("health_shell_failure") => {
                eprintln!("shell access denied");
                return ExitCode::FAILURE;
            }
            Ok("health_shell_timeout") => loop {
                thread::park();
            },
            Ok("health_invalid_uid") => println!("not-a-uid"),
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 5 && args[0] == "-s" && args[2..] == ["exec-out", "screencap", "-p"] {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected screenshot serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("screenshot") => io::stdout().write_all(SCREENSHOT_PNG).unwrap(),
            Ok("screenshot_invalid") => io::stdout().write_all(b"not a PNG").unwrap(),
            Ok("screenshot_failure") => {
                eprintln!("device offline");
                return ExitCode::FAILURE;
            }
            Ok("screenshot_timeout") => loop {
                thread::park();
            },
            Ok("screenshot_large") => {
                let _ = io::stdout().write_all(&vec![b'x'; 16 * 1024 * 1024 + 1]);
                loop {
                    thread::park();
                }
            }
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 4
        && args[0] == "-s"
        && args[2] == "shell"
        && args[3].contains("uiautomator dump")
    {
        let hierarchy = match args[1].as_str() {
            "emulator-5554" => {
                include_str!("../fixtures/screen/emulator-5554-window_dump.xml")
            }
            "emulator-5556" => {
                include_str!("../fixtures/screen/emulator-5556-window_dump.xml")
            }
            _ => {
                eprintln!("Unexpected UI hierarchy serial argument");
                return ExitCode::FAILURE;
            }
        };
        let script = &args[3];
        if !script.contains("cat") || !script.contains("trap 'rm -f") {
            eprintln!("UI hierarchy script must read and clean up its temporary file");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("ui_elements") => print!("{hierarchy}"),
            Ok("ui_elements_malformed") => print!("not xml"),
            Ok("ui_elements_failure") => {
                eprintln!("uiautomator unavailable");
                return ExitCode::FAILURE;
            }
            Ok("ui_elements_timeout") => loop {
                thread::park();
            },
            Ok("ui_elements_large") => {
                let _ = io::stdout().write_all(&vec![b'x'; 1024 * 1024 + 1]);
                loop {
                    thread::park();
                }
            }
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 5 && args[0] == "-s" && args[2..4] == ["shell", "pidof"] {
        if args[1] != "emulator-5554" || args[4] != "com.github.bschramke.myfinance" {
            eprintln!("Unexpected pidof serial or package argument");
            return ExitCode::FAILURE;
        }
        let mode = env::var("ANDROID_MCP_TEST_ADB_MODE").unwrap_or_default();
        let trace = env::var_os("ANDROID_MCP_TEST_PACKAGE_TRACE").unwrap();
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trace)
                .unwrap(),
            "pidof"
        )
        .unwrap();
        match mode.as_str() {
            "logcat_package" | "logcat_package_combined" => println!("1000"),
            "logcat_package_fresh" => {
                let count = fs::read_to_string(&trace)
                    .unwrap()
                    .lines()
                    .filter(|line| *line == "pidof")
                    .count();
                println!("{}", if count == 1 { 1000 } else { 2000 });
            }
            "logcat_package_absent" => return ExitCode::FAILURE,
            "logcat_package_empty" => {}
            "logcat_package_offline" => {
                eprintln!("device offline");
                return ExitCode::FAILURE;
            }
            "logcat_package_unsupported" => {
                eprintln!("pidof: not found");
                return ExitCode::from(127);
            }
            "logcat_package_invalid" => println!("not-a-pid"),
            "logcat_package_zero" => println!("0"),
            "logcat_package_multiple" => println!("1000 2000"),
            "logcat_package_invalid_utf8" => {
                io::stdout().write_all(&[0xff, 0xfe]).unwrap();
            }
            "logcat_package_timeout" => loop {
                thread::park();
            },
            "logcat_package_stdout_large" => {
                let _ = io::stdout().write_all(&vec![b'1'; 8192]);
                loop {
                    thread::park();
                }
            }
            "logcat_package_stderr_large" => {
                let _ = io::stderr().write_all(&vec![b'e'; 20 * 1024]);
                loop {
                    thread::park();
                }
            }
            "logcat_package_budget" => {
                thread::sleep(std::time::Duration::from_secs(3));
                println!("1000");
            }
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() >= 8
        && args[0] == "-s"
        && args[2..5] == ["logcat", "-d", "-t"]
        && args[6..8] == ["-v", "threadtime"]
    {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected logcat serial argument");
            return ExitCode::FAILURE;
        }
        let mode = env::var("ANDROID_MCP_TEST_ADB_MODE").unwrap_or_default();
        let package_pid = if mode.starts_with("logcat_package") {
            let trace = env::var_os("ANDROID_MCP_TEST_PACKAGE_TRACE").unwrap();
            let history = fs::read_to_string(&trace).unwrap_or_default();
            if history.lines().last() != Some("pidof") {
                eprintln!("Logcat must be preceded by pidof");
                return ExitCode::FAILURE;
            }
            let pid = if mode == "logcat_package_fresh"
                && history.lines().filter(|line| *line == "pidof").count() > 1
            {
                "--pid=2000"
            } else {
                "--pid=1000"
            };
            writeln!(
                OpenOptions::new().append(true).open(&trace).unwrap(),
                "logcat"
            )
            .unwrap();
            Some(pid)
        } else {
            None
        };
        let valid_lines = match mode.as_str() {
            "logcat_explicit" => args[5] == "17",
            "logcat_bounds" => args[5] == "1" || args[5] == "2000",
            _ => args[5] == "200",
        };
        if !valid_lines {
            eprintln!("Unexpected logcat line limit");
            return ExitCode::FAILURE;
        }
        let expected_filters: &[&str] = match mode.as_str() {
            "logcat_package" | "logcat_package_budget" | "logcat_package_fresh" => {
                &[package_pid.unwrap()]
            }
            "logcat_package_combined" => {
                &["--pid=1000", "ExampleApp:W", "AudioController:W", "*:S"]
            }
            "logcat_pid" => &["--pid=1000"],
            "logcat_pid_max" => &["--pid=2147483647"],
            "logcat_tags" => &["ExampleApp:V", "AudioController:V", "*:S"],
            "logcat_verbose" => &["*:V"],
            "logcat_debug" => &["*:D"],
            "logcat_info" => &["*:I"],
            "logcat_warn" => &["*:W"],
            "logcat_error" => &["*:E"],
            "logcat_fatal" => &["*:F"],
            "logcat_pid_tags" => &["--pid=1000", "ExampleApp:V", "*:S"],
            "logcat_pid_priority" => &["--pid=1000", "*:E"],
            "logcat_tags_priority" => &["ExampleApp:W", "AudioController:W", "*:S"],
            "logcat_combined" => &["--pid=2210", "ErrorProcessor:W", "AudioController:W", "*:S"],
            _ => &[],
        };
        if args[8..] != *expected_filters {
            eprintln!("Unexpected logcat filter arguments");
            return ExitCode::FAILURE;
        }
        match mode.as_str() {
            "logcat_package" | "logcat_package_fresh" | "logcat_package_combined" => {
                print!("{}", include_str!("../fixtures/logcat/threadtime.txt"));
            }
            "logcat_package_budget" => {
                thread::sleep(std::time::Duration::from_secs(3));
                print!("{}", include_str!("../fixtures/logcat/threadtime.txt"));
            }
            "logcat"
            | "logcat_explicit"
            | "logcat_bounds"
            | "logcat_pid"
            | "logcat_pid_max"
            | "logcat_tags"
            | "logcat_verbose"
            | "logcat_debug"
            | "logcat_info"
            | "logcat_warn"
            | "logcat_error"
            | "logcat_fatal"
            | "logcat_pid_tags"
            | "logcat_pid_priority"
            | "logcat_tags_priority"
            | "logcat_combined" => {
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
            "logcat_large_exit" => {
                let _ = io::stdout().write_all(&vec![b'x'; 256 * 1024 + 4096]);
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
    if args.len() == 5 && args[0] == "-s" && args[2] == "shell" && args[3..] == ["wm", "size"] {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected display-info serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("display_timeout") => loop {
                thread::park();
            },
            Ok("display_failure") | Ok("info_display_failure") => {
                eprintln!("wm size unavailable");
                return ExitCode::FAILURE;
            }
            Ok("display_unknown") => println!("Physical size: unknown"),
            _ => println!("Physical size: 1080x2400\nOverride size: 720x1600"),
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 5 && args[0] == "-s" && args[2] == "shell" && args[3..] == ["wm", "density"] {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected display-info serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("display_unknown") => println!("Physical density: unknown"),
            _ => println!("Physical density: 420\nOverride density: 440"),
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 6 && args[0] == "-s" && args[2..] == ["shell", "dumpsys", "window", "displays"]
    {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected display-info serial argument");
            return ExitCode::FAILURE;
        }
        let rotation = if env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() == Ok("display_unknown")
        {
            "ROTATION_UNKNOWN"
        } else {
            "ROTATION_1"
        };
        println!("Display: mDisplayId=0\nmRotation={rotation}");
        return ExitCode::SUCCESS;
    }
    if args.len() == 4 && args[0] == "-s" && args[2..] == ["shell", "getprop"] {
        if args[1] != "emulator-5554" && args[1] != "serial;not-a-shell-command" {
            eprintln!("Unexpected serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("info") => print!("{}", include_str!("../fixtures/getprop/synthetic.txt")),
            Ok("info_display_failure") => {
                print!("{}", include_str!("../fixtures/getprop/synthetic.txt"))
            }
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
    let is_app_listing = (args.len() == 6
        && args[0] == "-s"
        && args[2..] == ["shell", "pm", "list", "packages"])
        || (args.len() == 7
            && args[0] == "-s"
            && args[2..6] == ["shell", "pm", "list", "packages"]
            && args[6] == "-3");
    if is_app_listing {
        if args[1] != "emulator-5554"
            && args[1] != "emulator-5556"
            && args[1] != "serial;not-a-shell-command"
        {
            eprintln!("Unexpected app-list serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("apps") => println!("package:org.example\npackage:com.example.app\n"),
            Ok("apps_emulator_5554") if args[1] == "emulator-5554" => {
                print!("{}", include_str!("../fixtures/apps/emulator-5554-packages-list.txt"))
            }
            Ok("apps_emulator_5556") if args[1] == "emulator-5556" => {
                print!("{}", include_str!("../fixtures/apps/emulator-5556-packages-list.txt"))
            }
            Ok("apps_exclude_system") if args.len() == 7 => {
                println!("package:com.dhl.hasci.clickdummy\npackage:com.dhl.hasci.clickdummy.test\npackage:com.github.bschramke.myfinance\n")
            }
            Ok("apps_exclude_system") => {
                eprintln!("Expected pm list packages -3 when excluding system apps");
                return ExitCode::FAILURE;
            }
            Ok("apps_empty") => {}
            Ok("apps_malformed") => println!("not package output"),
            Ok("apps_invalid_utf8") => {
                io::stdout().write_all(&[0xff, 0xfe]).unwrap();
            }
            Ok("apps_failure") => {
                eprintln!("package manager unavailable");
                return ExitCode::FAILURE;
            }
            Ok("apps_timeout") => loop {
                thread::park();
            },
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 6
        && args[0] == "-s"
        && args[2..5] == ["shell", "dumpsys", "package"]
    {
        if args[1] != "emulator-5554" || args[5] != "com.example.app" {
            eprintln!("Unexpected app-info serial or package argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("app_info") => print!("{}", include_str!("../fixtures/apps/app-info-dumpsys.txt")),
            Ok("app_info_missing") => println!("Unable to find package: com.example.app"),
            Ok("app_info_failure") => {
                eprintln!("dumpsys unavailable");
                return ExitCode::FAILURE;
            }
            Ok("app_info_timeout") => loop {
                thread::park();
            },
            _ => return ExitCode::FAILURE,
        }
        return ExitCode::SUCCESS;
    }
    if args.len() == 6
        && args[0] == "-s"
        && args[2..] == ["shell", "dumpsys", "activity", "activities"]
    {
        if args[1] != "emulator-5554" && args[1] != "emulator-5556" {
            eprintln!("Unexpected foreground-app serial argument");
            return ExitCode::FAILURE;
        }
        match env::var("ANDROID_MCP_TEST_ADB_MODE").as_deref() {
            Ok("foreground_mresumed") => {
                println!("mResumedActivity: ActivityRecord{{abc u0 com.example.app/.MainActivity t42}}");
            }
            Ok("foreground_resumed") => {
                println!("ResumedActivity: ActivityRecord{{abc u0 com.example.app/com.example.app.Home t42}}");
            }
            Ok("foreground_none") => println!("ACTIVITY MANAGER ACTIVITIES"),
            Ok("foreground_failure") => {
                eprintln!("activity dump unavailable");
                return ExitCode::FAILURE;
            }
            Ok("foreground_timeout") => loop {
                thread::park();
            },
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
        Ok(
            "devices"
            | "health_online"
            | "health_root"
            | "health_shell_failure"
            | "health_shell_timeout"
            | "health_invalid_uid",
        ) => {
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
