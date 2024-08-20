use clap::Parser;
use couchy::config::get_config;
use couchy::config::AppConfig;
use couchy::view::*;
use eframe::egui;
use std::error::Error;
use tokio::runtime::Runtime;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = 0)]
    nox: u8,
    #[arg(short, long, default_value = "none")]
    save: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    //println!("{:?}", args);

    if args.nox != 1 {
        let native_options = eframe::NativeOptions::default();
        /*
            let native_options = eframe::NativeOptions {
                renderer: eframe::Renderer::Wgpu,
                ..Default::default()
        };
         */
        let _loop = eframe::run_native(
            "Couchy",
            native_options,
            Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
        );
    } else {
        println!("...run console");
        let config = get_config();
        if args.save == "all_design" {
            save_all_design(&config).await;
        } else if args.save == "all_server_design" {
            save_all_server_design(&config).await;
        }
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
    window_name: String,
}

impl MyEguiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    fn get_config(&self) -> AppConfig {
        let config = get_config();
        return config;
    }

    fn perform(log_lines: String, ctx: egui::Context) -> Result<(), Box<dyn Error>> {
        // call async from egui https://github.com/veto8/egui-tokio-example/blob/main/src/main.rs
        tokio::spawn(async move {
            let config = get_config();
            println!("...perform");
            save_all_server_design(&config).await;
        });
        Ok(())
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

            ui.heading(&self.window_name);

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

            if ui.button("Perform").clicked() {
                self.log_lines = self.window_name.to_string();
                let config = MyEguiApp::perform(self.log_lines.clone(), ctx.clone());
                //                save_all_server_design(&config).await;
            }

            ui.label("Log".to_string());
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut self.log_lines),
            );
        });

        // https://docs.rs/egui/latest/egui/struct.Ui.html
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Couchy", |ui| {
                    if ui.button("About Couchy").clicked() {
                        self.window_name = "About Couchy".to_string();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Settings").clicked() {
                        self.window_name = "Settings".to_string();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit Couchy").clicked() {
                        std::process::abort();
                    }
                });

                ui.menu_button("Views", |ui| {
                    if ui.button("Save all_design").clicked() {
                        let config = self.get_config();
                        self.host = config.host;
                        self.user = config.user;
                        self.database = config.database;
                        self.password = config.password;
                        self.window_name = "save_all_design".to_string();
                        ui.close_menu();
                    }

                    if ui.button("Save all_server_design").clicked() {
                        let config = self.get_config();
                        self.host = config.host;
                        self.user = config.user;
                        self.database = config.database;
                        self.password = config.password;
                        self.window_name = "save_all_server_design".to_string();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Help").clicked() {
                        self.window_help_open = true;
                        self.window_name = "Help".to_string();
                        ui.close_menu();
                    }
                });
            })
        });
    }
}
