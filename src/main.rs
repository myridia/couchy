use clap::Parser;
//use couchy::view::*;
use eframe::egui;
use homedir::my_home;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long, default_value_t = 0)]
    nox: u8,
}

fn main() {
    let args = Args::parse();
    //println!("...start {}!", args.nox);
    //let home = my_home().unwrap().unwrap();
    //let home_config = &format!("{0}/config.toml", home.display());
    //println!("{}", Path::new(home_config).exists());
    //    let config = PathBuf::from_str(&format!("{0}/config.toml", home.display()));
    if args.nox != 1 {
        //let native_options = eframe::NativeOptions::default();
        let native_options = eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        };
        eframe::run_native(
            "Couchy",
            native_options,
            Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
        );
    }
}

#[derive(Default)]
struct MyEguiApp {
    host: String,
    database: String,
    user: String,
    password: String,
    log_lines: String,
    window_help_open: bool,
    window_about_open: bool,
}

impl MyEguiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        Self::default()
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        /*
        self.host = env!("host").to_string();
        self.database = env!("database").to_string();
        self.user = env!("user").to_string();
        self.password = env!("password").to_string();
        */
        if self.window_help_open {
            egui::Window::new("Help")
                .open(&mut self.window_help_open)
                .show(ctx, |ui| {
                    ui.label("contents");
                });
        }

        if self.window_about_open {
            egui::Window::new("About")
                .open(&mut self.window_about_open)
                .show(ctx, |ui| {
                    ui.label("contents");
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(20.0);

            ui.heading("Couchy");

            ui.label("Host".to_string());
            let _database = ui.add(
                egui::TextEdit::singleline(&mut self.host)
                    .hint_text("Host")
                    .desired_width(f32::INFINITY)
                    .password(false),
            );

            ui.label("Database".to_string());
            let _database = ui.add(
                egui::TextEdit::singleline(&mut self.database)
                    .hint_text("Database")
                    .desired_width(f32::INFINITY)
                    .password(false),
            );

            ui.label("User".to_string());
            let _user = ui.add(
                egui::TextEdit::singleline(&mut self.user)
                    .hint_text("User")
                    .desired_width(f32::INFINITY)
                    .password(false),
            );

            ui.label("Password".to_string());
            let _password = ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .hint_text("Password")
                    .desired_width(f32::INFINITY)
                    .password(false),
            );

            if ui.button("Connect").clicked() {
                self.log_lines = "xxxxxxxxx".to_string();
            }

            ui.label("Log".to_string());
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut self.log_lines),
            );
        });

        //https://docs.rs/egui/latest/egui/struct.Ui.html
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Commands", |ui| {
                    if ui.button("Save All Views").clicked() {
                        ui.close_menu();
                    }

                    if ui.button("Get Doc").clicked() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Cut").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Copy").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Paste").clicked() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Help").clicked() {
                        self.window_help_open = true;
                        ui.close_menu();
                    }
                    if ui.button("About").clicked() {
                        self.window_about_open = true;
                        ui.close_menu();
                    }
                });
                if ui.button("Exit").clicked() {
                    self.log_lines = "close".to_string();
                    std::process::abort();
                }
            })
        });
    }
}
