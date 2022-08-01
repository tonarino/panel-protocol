use anyhow::Result;
use eframe::run_native;
use std::{
    env,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
mod app;
mod panel;

fn print_usage(args: &[String]) {
    println!("Usage: {} <tty_port>", args[0]);
    println!();
    println!("The program initiates a serial connection with the device specified by the ");
    println!("tty_port, and prints every Report that comes in");
    println!();
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        print_usage(&args);
        return Ok(());
    }

    let port = &args[1];
    let (report_tx, report_rx) = std::sync::mpsc::channel();
    let (command_tx, command_rx) = std::sync::mpsc::channel();

    let should_exit = Arc::new(AtomicBool::new(false));
    thread::spawn({
        let mut panel = panel::Panel::new(port)?;
        let should_exit = should_exit;
        move || loop {
            match panel.poll() {
                Ok(reports) => {
                    for report in reports {
                        println!("New serial message: {:?}", &report);
                        report_tx.send(report).unwrap();
                    }
                },
                Err(e) => {
                    eprintln!("Failed to poll reports: {}", e);
                    should_exit.store(true, Ordering::SeqCst);
                    return;
                },
            }

            while let Ok(command) = command_rx.try_recv() {
                panel.send(&command).unwrap();
            }
            thread::sleep(Duration::from_micros(50));
        }
    });

    let app = app::App::new(report_rx, command_tx);

    run_native(Box::new(app), Default::default());
}
