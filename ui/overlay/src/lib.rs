//! egui frontend. Native input, rendering, and runtime communication are pending.

/// Draw the initial disconnected status surface in a host-provided egui context.
pub fn show_disconnected(context: &egui::Context, open: &mut bool) {
    egui::Window::new("ReScaleFrame")
        .open(open)
        .show(context, |ui| {
            ui.heading("No game session connected");
            ui.label("Runtime settings will appear when a supported game session is ready.");
        });
}
