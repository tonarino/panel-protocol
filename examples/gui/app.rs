use std::{
    collections::VecDeque,
    num::NonZeroU16,
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};

use eframe::{
    egui::{self, FontDefinitions, FontFamily, ScrollArea, Vec2},
    epi::{self, Storage},
};
use panel_protocol::{Command, PulseMode, Report};

const SHOW_LAST_COMMAND_NUM: usize = 15;

#[derive(Clone, Copy, PartialEq)]
struct LedState {
    r: u8,
    g: u8,
    b: u8,
    if_breathing_interval_ms: u16,
    pulse_mode: PulseMode,
}

impl Default for LedState {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            if_breathing_interval_ms: 4000,
            pulse_mode: PulseMode::Solid,
        }
    }
}

impl From<LedState> for Command {
    fn from(led_state: LedState) -> Command {
        Command::Led {
            r: led_state.r,
            g: led_state.g,
            b: led_state.b,
            pulse_mode: led_state.pulse_mode,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
struct LightState {
    brightness: u16,
    temperature: u16,
}

pub struct App {
    report_rx: Receiver<Report>,
    command_tx: Sender<Command>,
    led_state: LedState,
    light_state: [LightState; 2],
    last_recv_reports: VecDeque<Report>,
    kill_updater: Option<Sender<()>>,
}

impl App {
    pub fn new(report_rx: Receiver<Report>, command_tx: Sender<Command>) -> Self {
        Self {
            report_rx,
            command_tx,
            led_state: Default::default(),
            light_state: Default::default(),
            last_recv_reports: VecDeque::new(),
            kill_updater: None,
        }
    }

    fn led_configuration_section(&mut self, ui: &mut eframe::egui::Ui) {
        ui.add(
            egui::Slider::new(&mut self.led_state.r, 0..=255).text("LED Red").clamp_to_range(true),
        );
        ui.add(
            egui::Slider::new(&mut self.led_state.g, 0..=255)
                .text("LED Green")
                .clamp_to_range(true),
        );
        ui.add(
            egui::Slider::new(&mut self.led_state.b, 0..=255).text("LED Blue").clamp_to_range(true),
        );

        // Pulse mode
        egui::ComboBox::from_label("Pulse Mode")
            .selected_text(format!("{:?}", self.led_state.pulse_mode))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.led_state.pulse_mode, PulseMode::Solid, "Solid");
                ui.selectable_value(
                    &mut self.led_state.pulse_mode,
                    PulseMode::DialTurn,
                    "DialTurn",
                );
                ui.selectable_value(
                    &mut self.led_state.pulse_mode,
                    PulseMode::Breathing {
                        interval_ms: NonZeroU16::new(self.led_state.if_breathing_interval_ms)
                            .unwrap(),
                    },
                    "Breathing",
                );
            });

        // Duration for pulse mode if breathing
        ui.scope(|ui| {
            ui.set_visible(matches!(self.led_state.pulse_mode, PulseMode::Breathing { .. }));

            let response = ui.add(
                egui::Slider::new(&mut self.led_state.if_breathing_interval_ms, 1..=u16::MAX)
                    .text("Breathing (half) interval (ms)")
                    .clamp_to_range(true),
            );
            if response.changed() {
                self.led_state.pulse_mode = PulseMode::Breathing {
                    interval_ms: NonZeroU16::new(self.led_state.if_breathing_interval_ms).unwrap(),
                };
            }
        });
    }

    fn lighting_configuration_section(&mut self, ui: &mut eframe::egui::Ui) {
        ui.label("Front Lights");
        ui.group(|ui| {
            ui.add(
                egui::Slider::new(&mut self.light_state[0].brightness, 0..=u16::MAX)
                    .text("Brightness")
                    .clamp_to_range(true),
            );
            ui.add(
                egui::Slider::new(&mut self.light_state[0].temperature, 0..=u16::MAX)
                    .text("Temperature")
                    .clamp_to_range(true),
            );
        });
        ui.label("Back Lights");
        ui.group(|ui| {
            ui.add(
                egui::Slider::new(&mut self.light_state[1].brightness, 0..=u16::MAX)
                    .text("Brightness")
                    .clamp_to_range(true),
            );
            ui.add(
                egui::Slider::new(&mut self.light_state[1].temperature, 0..=u16::MAX)
                    .text("Temperature")
                    .clamp_to_range(true),
            );
        });
    }

    fn other_commands_section(&mut self, ui: &mut eframe::egui::Ui) {
        if ui.button(format!("Send {:?} command", Command::Bootload)).clicked() {
            self.command_tx.send(Command::Bootload).unwrap();
        }
    }

    fn serial_monitor_section(&mut self, ui: &mut eframe::egui::Ui) {
        ui.group(|ui| {
            let commands_strings = self
                .last_recv_reports
                .iter()
                .map(|report| format!("New serial message received: {:?}", report))
                .collect::<Vec<_>>();
            ui.add(egui::Label::new(commands_strings.join("\n")).code())
        });
    }
}

impl epi::App for App {
    fn setup(
        &mut self,
        _ctx: &eframe::egui::CtxRef,
        _frame: &mut epi::Frame<'_>,
        _: Option<&dyn Storage>,
    ) {
        // Add another thread to force a repaint on new reports being received, forwards those reports
        let (report_tx, mut report_rx) = channel();
        let (kill_updater_tx, kill_updater_rx) = channel();
        std::mem::swap(&mut self.report_rx, &mut report_rx);
        self.kill_updater = Some(kill_updater_tx);
        let repaint_signal = _frame.repaint_signal().clone();
        std::thread::spawn(move || loop {
            if kill_updater_rx.try_recv().is_ok() {
                println!("Killed updater thread.");
                break;
            }
            while let Ok(report) = report_rx.try_recv() {
                report_tx.send(report).unwrap();
                repaint_signal.request_repaint();
            }
            std::thread::sleep(Duration::from_millis(1));
        });

        // Update the led on startup
        self.command_tx.send(self.led_state.into()).unwrap();

        // Setup some fonts
        let mut fonts = FontDefinitions::default();
        fonts.family_and_size.insert(egui::TextStyle::Body, (FontFamily::Proportional, 18.0));
        fonts.family_and_size.insert(egui::TextStyle::Button, (FontFamily::Proportional, 18.0));
        fonts.family_and_size.insert(egui::TextStyle::Monospace, (FontFamily::Monospace, 18.0));
        fonts.family_and_size.insert(egui::TextStyle::Heading, (FontFamily::Proportional, 24.0));
        _ctx.set_fonts(fonts);
    }

    fn update(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
        let current_led_state = self.led_state.clone();
        let current_light_state = self.light_state.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Ok(report) = self.report_rx.try_recv() {
                self.last_recv_reports.push_back(report);
                while self.last_recv_reports.len() > SHOW_LAST_COMMAND_NUM {
                    self.last_recv_reports.pop_front();
                }
            }

            ScrollArea::auto_sized().show(ui, |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 300.0;
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                ui.spacing_mut().button_padding = Vec2::new(10.0, 10.0);
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Panel Configurator");
                    // RGB sliders
                    ui.separator();
                    ui.collapsing("RGB LED Configuration", |ui| self.led_configuration_section(ui));

                    // Lighting
                    ui.separator();
                    ui.collapsing("Lighting", |ui| self.lighting_configuration_section(ui));

                    // Booloader command
                    ui.separator();
                    ui.collapsing("Other Commands", |ui| self.other_commands_section(ui));

                    // Show last few commands
                    ui.separator();
                    ui.collapsing(
                        format!("Serial Monitor (last {} messages)", SHOW_LAST_COMMAND_NUM),
                        |ui| self.serial_monitor_section(ui),
                    );

                    // Warn if debug build
                    egui::warn_if_debug_build(ui);
                });
            });
        });

        if self.led_state != current_led_state {
            self.command_tx.send(self.led_state.into()).unwrap();
        }

        if self.light_state != current_light_state {
            for (target, state) in self.light_state.iter().enumerate() {
                let target = target as u8;
                self.command_tx
                    .send(Command::Brightness { target, value: state.brightness })
                    .unwrap();
                self.command_tx
                    .send(Command::Temperature { target, value: state.temperature })
                    .unwrap();
            }
        }
    }

    fn name(&self) -> &str {
        "Panel Configurator"
    }

    fn on_exit(&mut self) {
        if let Some(kill_updater) = &self.kill_updater {
            kill_updater.send(()).unwrap();
        }
    }
}
