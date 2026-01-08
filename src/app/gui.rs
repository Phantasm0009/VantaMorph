#[cfg(not(target_arch = "wasm32"))]
use super::DRAWING_ALPHA;

use super::GuiMode;
use super::VantaMorphApp;
use crate::app::DEFAULT_RESOLUTION;
use crate::app::calculate;
use crate::app::calculate::ProgressMsg;
use crate::app::calculate::util::CropScale;
use crate::app::calculate::util::GenerationSettings;
use crate::app::calculate::util::SourceImg;
use crate::app::gif_recorder::GIF_FRAMERATE;
use crate::app::gif_recorder::GIF_RESOLUTION;
use crate::app::gif_recorder::GifStatus;
use crate::app::preset::Preset;
use crate::app::preset::UnprocessedPreset;
use eframe::App;
use eframe::Frame;
use egui::Color32;
use egui::Modal;
use egui::TextureHandle;
use egui::Window;
use image::buffer::ConvertBuffer;
use image::imageops;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use uuid::Uuid;

// #[cfg(not(target_arch = "wasm32"))]
// use std::thread as wasm_thread;

#[derive(Default)]
struct GuiImageCache {
    source_preview: Option<egui::TextureHandle>,
    target_preview: Option<egui::TextureHandle>,
    overlap_preview: Option<egui::TextureHandle>,
}

/// UI mode: Simple for beginners, Pro for advanced users
#[derive(Clone, Copy, PartialEq, Default)]
pub enum UiMode {
    #[default]
    Simple,
    Pro,
}

/// Right panel tab selection
#[derive(Clone, Copy, PartialEq, Default)]
pub enum RightPanelTab {
    #[default]
    Presets,
    Motion,
    Quality,
}

/// Playback speed options
#[derive(Clone, Copy, PartialEq)]
pub enum PlaybackSpeed {
    Quarter, // 0.25x
    Half,    // 0.5x
    Normal,  // 1x
    Double,  // 2x
}

impl Default for PlaybackSpeed {
    fn default() -> Self {
        PlaybackSpeed::Normal
    }
}

impl PlaybackSpeed {
    fn multiplier(&self) -> f32 {
        match self {
            PlaybackSpeed::Quarter => 0.25,
            PlaybackSpeed::Half => 0.5,
            PlaybackSpeed::Normal => 1.0,
            PlaybackSpeed::Double => 2.0,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            PlaybackSpeed::Quarter => "0.25×",
            PlaybackSpeed::Half => "0.5×",
            PlaybackSpeed::Normal => "1×",
            PlaybackSpeed::Double => "2×",
        }
    }
}

/// Motion style for particle animation
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MotionStyle {
    #[default]
    Linear,
    Float,
    Swirl,
    Dust,
    MagnetSnap,
}

impl MotionStyle {
    fn label(&self) -> &'static str {
        match self {
            MotionStyle::Linear => "Linear",
            MotionStyle::Float => "Float",
            MotionStyle::Swirl => "Swirl",
            MotionStyle::Dust => "Dust",
            MotionStyle::MagnetSnap => "Magnet Snap",
        }
    }
}

/// Compare view mode
#[derive(Clone, Copy, PartialEq, Default)]
pub enum CompareView {
    #[default]
    None,
    BeforeAfter,
    Split,
}

pub(crate) struct GuiState {
    #[cfg(not(target_arch = "wasm32"))]
    pub last_mouse_pos: Option<(f32, f32)>,
    #[cfg(not(target_arch = "wasm32"))]
    pub drawing_color: [f32; 4],
    #[allow(dead_code)]
    mode: GuiMode,
    pub animate: bool,
    //pub fps_text: String,
    show_progress_modal: Option<Uuid>,
    last_progress: f32,
    process_cancelled: Arc<AtomicBool>,
    //pub currently_processing: Option<Preset>,
    pub presets: Vec<Preset>,
    //pub current_settings: GenerationSettings,
    configuring_generation: Option<(SourceImg, GenerationSettings, GuiImageCache)>,
    saved_config: Option<(SourceImg, GenerationSettings)>,
    pub current_preset: usize,
    pub current_preset_target: Option<SourceImg>,
    error_message: Option<String>,

    has_morphed_once: bool,

    /// Pending image to auto-morph (set when user drops/uploads an image for automatic processing)
    pub pending_auto_morph: Option<(String, SourceImg)>,

    /// When processing a preset click, this holds the index to replace instead of creating new
    pub replacing_preset_index: Option<usize>,

    /// Pending preset to process on next frame (for initial load)
    pub pending_preset_process: Option<usize>,

    /// Frame counter to delay initial processing until worker is ready (WASM)
    pub frames_since_start: u32,

    // === New UI State ===
    /// Simple vs Pro UI mode
    pub ui_mode: UiMode,

    /// Right panel tab selection (Pro mode)
    pub right_panel_tab: RightPanelTab,

    /// Playback speed multiplier
    pub playback_speed: PlaybackSpeed,

    /// Loop playback
    pub loop_playback: bool,

    /// Timeline position (0.0 to 1.0)
    pub timeline_position: f32,

    /// Is timeline being scrubbed
    pub scrubbing: bool,

    /// Source image for current morph (thumbnail)
    #[allow(dead_code)]
    pub source_thumbnail: Option<TextureHandle>,

    /// Target image for current morph (thumbnail)  
    #[allow(dead_code)]
    pub target_thumbnail: Option<TextureHandle>,

    /// Staged source image (before starting morph)
    pub staged_source: Option<(String, SourceImg)>,

    /// Staged target image (before starting morph, None = use default)
    pub staged_target: Option<(String, SourceImg)>,

    /// Texture handle for staged source preview
    pub staged_source_texture: Option<TextureHandle>,

    /// Texture handle for staged target preview
    pub staged_target_texture: Option<TextureHandle>,

    /// Show left panel
    pub show_left_panel: bool,

    /// Show right panel
    pub show_right_panel: bool,

    /// Lock target (keep target while testing different sources)
    pub lock_target: bool,

    /// Motion style
    pub motion_style: MotionStyle,

    /// Motion sliders
    pub swirl_amount: f32,
    pub turbulence: f32,
    pub snap_strength: f32,
    pub dissolve: f32,
    pub animation_duration: f32,

    /// Quality settings
    pub resolution: u32,
    pub edge_boost: bool,
    pub dither_enabled: bool,
    pub dither_strength: f32,

    /// Compare view mode
    pub compare_view: CompareView,
    pub split_position: f32,

    /// Show canvas overlays (particle count, fps, resolution)
    pub show_overlays: bool,

    /// Project name
    pub project_name: String,
}

impl GuiState {
    pub fn default(
        presets: Vec<Preset>,
        current_preset: usize,
        has_morphed_once: bool,
    ) -> GuiState {
        let current_preset_target = presets.get(current_preset).and_then(|preset| {
            preset.inner.target_img.as_ref().and_then(|data| {
                image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(
                    preset.inner.width,
                    preset.inner.height,
                    data.clone(),
                )
            })
        });

        GuiState {
            animate: true,
            //fps_text: String::new(),
            presets,
            mode: GuiMode::Transform,
            show_progress_modal: None,
            last_progress: 0.0,
            process_cancelled: Arc::new(AtomicBool::new(false)),
            #[cfg(not(target_arch = "wasm32"))]
            last_mouse_pos: None,
            #[cfg(not(target_arch = "wasm32"))]
            drawing_color: [0.0, 0.0, 0.0, DRAWING_ALPHA],
            //currently_processing: None,
            //current_settings: GenerationSettings::default(),
            configuring_generation: None,
            saved_config: None,
            current_preset,
            current_preset_target,
            error_message: None,
            has_morphed_once,
            pending_auto_morph: None,
            replacing_preset_index: None,
            pending_preset_process: Some(current_preset),
            frames_since_start: 0,
            // New UI state
            ui_mode: UiMode::Simple,
            right_panel_tab: RightPanelTab::Presets,
            playback_speed: PlaybackSpeed::Normal,
            loop_playback: true,
            timeline_position: 0.0,
            scrubbing: false,
            source_thumbnail: None,
            target_thumbnail: None,
            staged_source: None,
            staged_target: None,
            staged_source_texture: None,
            staged_target_texture: None,
            show_left_panel: true,
            show_right_panel: true,
            lock_target: false,
            motion_style: MotionStyle::Linear,
            swirl_amount: 0.0,
            turbulence: 0.0,
            snap_strength: 0.0,
            dissolve: 0.0,
            animation_duration: 3.0,
            resolution: 128,
            edge_boost: false,
            dither_enabled: false,
            dither_strength: 0.5,
            compare_view: CompareView::None,
            split_position: 0.5,
            show_overlays: true,
            project_name: String::from("Untitled Project"),
        }
    }

    fn show_progress_modal(&mut self, id: Uuid) {
        self.show_progress_modal = Some(id);
        #[cfg(target_arch = "wasm32")]
        hide_icons();
    }

    fn hide_progress_modal(&mut self) {
        self.show_progress_modal = None;
        #[cfg(target_arch = "wasm32")]
        show_icons();
    }

    fn show_error(&mut self, msg: String) {
        self.error_message = Some(msg);
    }

    fn hide_error(&mut self) {
        self.error_message = None;
    }
}

#[cfg(target_arch = "wasm32")]
fn show_icons() {
    use wasm_bindgen::JsCast;
    // show .bottom-left-icons class after processing
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(icons) = document.query_selector(".bottom-left-icons").ok().flatten() {
            let _ = icons
                .dyn_ref::<web_sys::HtmlElement>()
                .map(|e| e.style().set_property("display", "flex"));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn hide_icons() {
    use wasm_bindgen::JsCast;
    // hide .bottom-left-icons class while processing
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(icons) = document.query_selector(".bottom-left-icons").ok().flatten() {
            let _ = icons
                .dyn_ref::<web_sys::HtmlElement>()
                .map(|e| e.style().set_property("display", "none"));
        }
    }
}

/// Check if an image was dropped via drag-and-drop or pasted from clipboard (WASM only)
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
fn check_dropped_image() -> Option<(String, SourceImg)> {
    use web_sys::js_sys;

    let window = web_sys::window()?;

    // Check if droppedImageData exists and has data
    let dropped_data = js_sys::Reflect::get(&window, &"droppedImageData".into()).ok()?;
    if dropped_data.is_null() || dropped_data.is_undefined() {
        return None;
    }

    // Get the name
    let name_val = js_sys::Reflect::get(&window, &"droppedImageName".into()).ok()?;
    let name = name_val
        .as_string()
        .unwrap_or_else(|| "dropped_image".to_string());

    // Convert to Uint8Array and then to Vec<u8>
    let uint8_array = js_sys::Uint8Array::from(dropped_data);
    let data: Vec<u8> = uint8_array.to_vec();

    // Clear the dropped data immediately
    let _ = js_sys::Reflect::set(
        &window,
        &"droppedImageData".into(),
        &wasm_bindgen::JsValue::NULL,
    );
    let _ = js_sys::Reflect::set(
        &window,
        &"droppedImageName".into(),
        &wasm_bindgen::JsValue::NULL,
    );

    // Try to load the image
    match image::load_from_memory(&data) {
        Ok(img) => Some((name, img.to_rgb8())),
        Err(e) => {
            web_sys::console::error_1(&format!("Failed to load dropped image: {}", e).into());
            None
        }
    }
}

impl App for VantaMorphApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "presets", &self.gui.presets);
        eframe::set_value(storage, "has_morphed_once", &self.gui.has_morphed_once);
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut Frame) {
        let Some(rs) = frame.wgpu_render_state() else {
            return;
        };

        let device = &rs.device;
        // Resize handling (match the egui "central panel" size)
        //let available = ctx.available_rect();
        // let target_size = (
        //     available.width().max(1.0) as u32,
        //     available.height().max(1.0) as u32,
        // );
        // if target_size != self.size {
        //     self.resize(rs, target_size);
        // }

        // Ensure texture is registered exactly once per allocation
        self.ensure_registered_texture(
            rs,
            if self.size.0 < 512 {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            },
        );

        #[cfg(target_arch = "wasm32")]
        self.ensure_worker(ctx);

        // Check for dropped/pasted images (WASM only)
        #[cfg(target_arch = "wasm32")]
        {
            if self.gui.pending_auto_morph.is_none()
                && self.gui.show_progress_modal.is_none()
                && self.gui.configuring_generation.is_none()
            {
                if let Some((name, img)) = check_dropped_image() {
                    self.gui.pending_auto_morph = Some((name, img));
                }
            }
        }

        // Run GPU pipeline
        if let Some(img) = &self.preview_image {
            // show image
            let img = if img.width() != self.size.0 || img.height() != self.size.1 {
                &image::imageops::resize(
                    img,
                    self.size.0,
                    self.size.1,
                    image::imageops::FilterType::Nearest,
                )
            } else {
                img
            };
            let rgba: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = img.convert();
            let rgba = rgba.into_raw();
            rs.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.color_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * self.size.0),
                    rows_per_image: Some(self.size.1),
                },
                wgpu::Extent3d {
                    width: self.size.0,
                    height: self.size.1,
                    depth_or_array_layers: 1,
                },
            );
        } else {
            self.run_gpu(rs);

            if self.gui.animate {
                if self.gif_recorder.is_recording() {
                    if self.gif_recorder.no_inflight() {
                        if let Err(e) = self.get_color_image_data(device, &rs.queue) {
                            self.gif_recorder.status = GifStatus::Error(e.to_string());
                        }
                    }
                    match self.gif_recorder.try_write_frame() {
                        Err(e) => {
                            self.gif_recorder.status = GifStatus::Error(e.to_string());
                            self.gui.animate = false;
                        }
                        Ok(true) => {
                            for _ in 0..(60 / GIF_FRAMERATE) {
                                self.sim.update(&mut self.seeds, self.size.0);
                            }

                            self.gif_recorder.frame_count += 1;

                            if self.gif_recorder.should_stop() {
                                // finish recording
                                if !self.gif_recorder.finish(
                                    self.gif_recorder.get_name(self.sim.name(), self.reverse),
                                ) {
                                    // cancelled
                                    self.stop_recording_gif(device, &rs.queue);
                                }

                                self.gui.animate = false;
                            } else {
                                // queue next frame
                                if let Err(e) = self.get_color_image_data(device, &rs.queue) {
                                    self.gif_recorder.status = GifStatus::Error(e.to_string());
                                }
                            }
                        }

                        Ok(false) => { /* not ready yet */ }
                    }
                } else {
                    // Run multiple updates per frame for faster animation
                    // Adjust based on playback speed
                    let base_updates = 3;
                    let speed_mult = self.gui.playback_speed.multiplier();
                    let updates = ((base_updates as f32) * speed_mult).max(1.0) as usize;

                    for _ in 0..updates {
                        self.sim.update(&mut self.seeds, self.size.0);
                    }
                }
                rs.queue
                    .write_buffer(&self.seed_buf, 0, bytemuck::cast_slice(&self.seeds));
                // Update seed texture for WebGL compatibility
                self.update_seed_texture_data(&rs.queue, &self.seeds);
            }
        }

        // let dt = self.prev_frame_time.elapsed();
        // self.prev_frame_time = std::time::Instant::now();
        // self.gui.fps_text = format!(
        //     "{:5.2} ms/frame (~{:06.0} FPS)",
        //     dt.as_secs_f64() * 1000.0,
        //     1.0 / dt.as_secs_f64()
        // );

        let screen_width = ctx.available_rect().width();
        let is_landscape = screen_width > ctx.available_rect().height();
        let _mobile_layout = screen_width < 750.0;

        let baseline_zoom = if is_landscape { 1.4_f32 } else { 1.0_f32 };

        // Create textures for staged source/target images if needed
        if self.gui.staged_source.is_some() && self.gui.staged_source_texture.is_none() {
            if let Some((_, img)) = &self.gui.staged_source {
                // Resize to thumbnail size
                let thumb =
                    image::imageops::resize(img, 100, 100, image::imageops::FilterType::Triangle);
                let rgba: image::RgbaImage = image::DynamicImage::ImageRgb8(thumb).into_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.gui.staged_source_texture = Some(ctx.load_texture(
                    "staged_source_thumb",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }

        if self.gui.staged_target.is_some() && self.gui.staged_target_texture.is_none() {
            if let Some((_, img)) = &self.gui.staged_target {
                // Resize to thumbnail size
                let thumb =
                    image::imageops::resize(img, 100, 100, image::imageops::FilterType::Triangle);
                let rgba: image::RgbaImage = image::DynamicImage::ImageRgb8(thumb).into_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.gui.staged_target_texture = Some(ctx.load_texture(
                    "staged_target_thumb",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }

        // Handle pending auto-morph request (from drag-drop or auto-upload)
        if let Some((name, img)) = self.gui.pending_auto_morph.take() {
            let img = ensure_reasonable_size(img);
            let mut settings = GenerationSettings::default(Uuid::new_v4(), name.clone());

            // Apply preset target if available so we morph into preset's target, not default Obama
            if let Some(target) = &self.gui.current_preset_target {
                settings.set_raw_target(target.clone());
            }

            self.gui.show_progress_modal(settings.id);
            self.gui.saved_config = Some((img.clone(), settings.clone()));

            // Adjust proximity importance for consistency across resolutions
            settings.proximity_importance =
                (settings.proximity_importance as f32 / (settings.sidelen as f32 / 128.0)) as i64;

            self.gui
                .process_cancelled
                .store(false, std::sync::atomic::Ordering::Relaxed);

            let unprocessed = UnprocessedPreset {
                name: settings.name.clone(),
                width: img.width(),
                height: img.height(),
                source_img: img.into_raw(),
                target_img: None,
            };

            self.resize_textures(device, (settings.sidelen, settings.sidelen), false);

            #[cfg(target_arch = "wasm32")]
            {
                self.start_job(unprocessed, settings);
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                std::thread::spawn({
                    let mut tx = self.progress_tx.clone();
                    let cancelled = self.gui.process_cancelled.clone();
                    move || {
                        let result = calculate::process(unprocessed, settings, &mut tx, cancelled);
                        if let Err(err) = result {
                            tx.send(ProgressMsg::Error(err.to_string())).ok();
                        }
                    }
                });
            }
        }

        // Increment frame counter for WASM worker initialization delay
        self.gui.frames_since_start = self.gui.frames_since_start.saturating_add(1);

        // Handle pending preset processing (for initial load or preset clicks)
        // On WASM, wait a few frames for the worker to fully initialize
        #[cfg(target_arch = "wasm32")]
        let worker_ready = self.gui.frames_since_start > 10;
        #[cfg(not(target_arch = "wasm32"))]
        let worker_ready = true;

        if worker_ready {
            if let Some(preset_idx) = self.gui.pending_preset_process.take() {
                if let Some(preset) = self.gui.presets.get(preset_idx).cloned() {
                    if preset.inner.target_img.is_some() {
                        let source_img = image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(
                            preset.inner.width,
                            preset.inner.height,
                            preset.inner.source_img.clone(),
                        )
                        .unwrap();

                        let mut settings =
                            GenerationSettings::default(Uuid::new_v4(), preset.inner.name.clone());

                        // Set the preset's target image
                        if let Some(target_data) = &preset.inner.target_img {
                            if let Some(target_img) =
                                image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(
                                    preset.inner.width,
                                    preset.inner.height,
                                    target_data.clone(),
                                )
                            {
                                settings.set_raw_target(target_img);
                            }
                        }

                        self.gui.show_progress_modal(settings.id);
                        self.gui.saved_config = Some((source_img.clone(), settings.clone()));
                        self.gui.replacing_preset_index = Some(preset_idx);

                        settings.proximity_importance = (settings.proximity_importance as f32
                            / (settings.sidelen as f32 / 128.0))
                            as i64;

                        self.gui
                            .process_cancelled
                            .store(false, std::sync::atomic::Ordering::Relaxed);

                        let unprocessed = UnprocessedPreset {
                            name: settings.name.clone(),
                            width: source_img.width(),
                            height: source_img.height(),
                            source_img: source_img.into_raw(),
                            target_img: None,
                        };

                        self.resize_textures(device, (settings.sidelen, settings.sidelen), false);

                        #[cfg(target_arch = "wasm32")]
                        {
                            self.start_job(unprocessed, settings);
                        }

                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            std::thread::spawn({
                                let mut tx = self.progress_tx.clone();
                                let cancelled = self.gui.process_cancelled.clone();
                                move || {
                                    let result = calculate::process(
                                        unprocessed,
                                        settings,
                                        &mut tx,
                                        cancelled,
                                    );
                                    if let Err(err) = result {
                                        tx.send(ProgressMsg::Error(err.to_string())).ok();
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.ctx().set_zoom_factor(baseline_zoom);

            // === NEW TOP BAR DESIGN ===
            ui.horizontal(|ui| {
                // Left section: Logo + Project name
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🎨").size(20.0));
                    ui.label(egui::RichText::new("VantaMorph").strong().size(16.0));
                    ui.separator();

                    // Editable project name
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gui.project_name)
                            .desired_width(120.0)
                            .hint_text("Project name..."),
                    );
                });

                ui.separator();

                // Center section: Preset picker + Randomize
                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("preset_picker_top")
                        .width(150.0)
                        .selected_text({
                            let name = self.sim.name();
                            if name.chars().count() > 15 {
                                let truncated: String = name.chars().take(12).collect();
                                format!("{truncated}…")
                            } else {
                                name.clone()
                            }
                        })
                        .show_ui(ui, |ui| {
                            let mut clicked_preset: Option<(usize, Preset)> = None;

                            for (i, preset) in self.gui.presets.clone().into_iter().enumerate() {
                                let selected = i == self.gui.current_preset;
                                if ui.selectable_label(selected, &preset.inner.name).clicked() {
                                    clicked_preset = Some((i, preset));
                                }
                            }

                            if let Some((i, preset)) = clicked_preset {
                                // Trigger fresh morph calculation if preset has target
                                if preset.inner.target_img.is_some() {
                                    self.gui.pending_preset_process = Some(i);
                                } else {
                                    self.change_sim(device, &rs.queue, preset, i);
                                    self.gui.animate = true;
                                }
                                self.gui.current_preset = i;
                            }
                        });

                    // Randomize button
                    if ui
                        .button("🎲 Random")
                        .on_hover_text("Pick a random preset")
                        .clicked()
                    {
                        let random_idx =
                            (self.gui.frames_since_start as usize) % self.gui.presets.len();
                        if let Some(preset) = self.gui.presets.get(random_idx).cloned() {
                            if preset.inner.target_img.is_some() {
                                self.gui.pending_preset_process = Some(random_idx);
                            } else {
                                self.change_sim(device, &rs.queue, preset, random_idx);
                                self.gui.animate = true;
                            }
                            self.gui.current_preset = random_idx;
                        }
                    }
                });

                // Spacer
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right section: Export + Share + Settings

                    // Settings menu
                    ui.menu_button("⚙", |ui| {
                        ui.checkbox(&mut self.gui.show_overlays, "Show canvas overlays");
                        ui.checkbox(&mut self.gui.loop_playback, "Loop playback");
                        ui.separator();
                        if ui.button("Reset to defaults").clicked() {
                            self.gui.animation_duration = 3.0;
                            self.gui.swirl_amount = 0.0;
                            self.gui.turbulence = 0.0;
                            self.gui.resolution = 128;
                            ui.close();
                        }
                    });

                    // Share button (placeholder)
                    if ui
                        .button("🔗 Share")
                        .on_hover_text("Share this morph")
                        .clicked()
                    {
                        // TODO: Implement share functionality
                        #[cfg(target_arch = "wasm32")]
                        {
                            web_sys::window()
                                .unwrap()
                                .alert_with_message("Share feature coming soon!")
                                .ok();
                        }
                    }

                    // Export button (primary action)
                    let export_btn = egui::Button::new(egui::RichText::new("📤 Export").strong())
                        .fill(egui::Color32::from_rgb(70, 130, 180));
                    if ui.add(export_btn).on_hover_text("Export as GIF").clicked() {
                        if !self.gif_recorder.is_recording() {
                            self.gif_recorder.status = GifStatus::Recording;
                            self.gif_recorder.encoder = None;
                            if let Err(err) = self
                                .gif_recorder
                                .init_encoder(self.colors.read().unwrap().as_ref())
                            {
                                self.gif_recorder.status = GifStatus::Error(err.to_string());
                            } else {
                                self.resize_textures(
                                    device,
                                    (GIF_RESOLUTION, GIF_RESOLUTION),
                                    false,
                                );
                                self.reset_sim(device, &rs.queue);
                                self.gui.animate = true;
                                for _ in 0..20 {
                                    self.sim.update(&mut self.seeds, self.size.0);
                                }
                            }
                        }
                    }

                    ui.separator();

                    // Morph new image button (glows if user hasn't morphed once)
                    let morph_btn_response = if !self.gui.has_morphed_once {
                        let time = ui.input(|i| i.time);
                        let pulse = ((time * 2.0).sin() * 0.5 + 0.5) as f32;
                        let glow_color = egui::Color32::from_rgb(
                            (30.0 + pulse * 100.0) as u8,
                            (120.0 + pulse * 135.0) as u8,
                            (200.0 + pulse * 55.0) as u8,
                        );
                        ui.add(
                            egui::Button::new("✨ Upload Image")
                                .stroke(egui::Stroke::new(2.0, glow_color)),
                        )
                    } else {
                        ui.button("📁 Upload")
                    };

                    if morph_btn_response
                        .on_hover_text("Upload a new image to morph")
                        .clicked()
                    {
                        prompt_image(
                            "choose source image",
                            self,
                            |name: String, img: SourceImg, app: &mut VantaMorphApp| {
                                app.gui.pending_auto_morph = Some((name, img));
                            },
                        );
                    }
                });
            });
        });

        // === LEFT SIDE PANEL (Pro Mode) ===
        if self.gui.ui_mode == UiMode::Pro && self.gui.show_left_panel {
            egui::SidePanel::left("left_panel")
                .default_width(220.0)
                .min_width(180.0)
                .resizable(true)
                .frame(egui::Frame::group(&ctx.style()).inner_margin(egui::Margin::same(12)))
                .show(ctx, |ui| {
                    ui.heading("Inputs");
                    ui.separator();

                    // === SOURCE (Start) Card ===
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Source (Start)").strong());

                        // Thumbnail - show actual image if staged, otherwise placeholder
                        let thumb_size = egui::vec2(100.0, 100.0);

                        if let Some(tex) = &self.gui.staged_source_texture {
                            // Show actual thumbnail
                            ui.add(egui::Image::new((tex.id(), thumb_size)).corner_radius(4.0));

                            // Show name
                            if let Some((name, _)) = &self.gui.staged_source {
                                let display_name = if name.len() > 15 {
                                    format!("{}…", &name[..12])
                                } else {
                                    name.clone()
                                };
                                ui.label(egui::RichText::new(display_name).small().weak());
                            }
                        } else {
                            // Placeholder
                            let (rect, _resp) =
                                ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rect, 4.0, egui::Color32::from_gray(40));
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "📷\nDrop or Upload",
                                egui::FontId::proportional(14.0),
                                egui::Color32::GRAY,
                            );
                        }

                        ui.horizontal(|ui| {
                            if ui.small_button("📁 Upload").clicked() {
                                prompt_image(
                                    "choose source image",
                                    self,
                                    |name: String, img: SourceImg, app: &mut VantaMorphApp| {
                                        // Stage the source image (don't start morph yet)
                                        app.gui.staged_source = Some((name, img));
                                        app.gui.staged_source_texture = None; // Will be created on next frame
                                    },
                                );
                            }
                            if self.gui.staged_source.is_some() {
                                if ui.small_button("✕ Clear").clicked() {
                                    self.gui.staged_source = None;
                                    self.gui.staged_source_texture = None;
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);

                    // Swap button
                    let can_swap =
                        self.gui.staged_source.is_some() || self.gui.staged_target.is_some();
                    if ui
                        .add_enabled(can_swap, egui::Button::new("⇅ Swap Source ↔ Target"))
                        .clicked()
                    {
                        std::mem::swap(&mut self.gui.staged_source, &mut self.gui.staged_target);
                        std::mem::swap(
                            &mut self.gui.staged_source_texture,
                            &mut self.gui.staged_target_texture,
                        );
                    }

                    ui.add_space(8.0);

                    // === TARGET (End) Card ===
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Target (End)").strong());
                            // Lock toggle
                            let lock_icon = if self.gui.lock_target { "🔒" } else { "🔓" };
                            if ui
                                .small_button(lock_icon)
                                .on_hover_text("Lock target image")
                                .clicked()
                            {
                                self.gui.lock_target = !self.gui.lock_target;
                            }
                        });

                        // Thumbnail - show actual image if staged, otherwise placeholder for "default"
                        let thumb_size = egui::vec2(100.0, 100.0);

                        if let Some(tex) = &self.gui.staged_target_texture {
                            // Show actual thumbnail
                            ui.add(egui::Image::new((tex.id(), thumb_size)).corner_radius(4.0));

                            // Show name
                            if let Some((name, _)) = &self.gui.staged_target {
                                let display_name = if name.len() > 15 {
                                    format!("{}…", &name[..12])
                                } else {
                                    name.clone()
                                };
                                ui.label(egui::RichText::new(display_name).small().weak());
                            }
                        } else {
                            // Placeholder showing "Default" or "Use Preset Target"
                            let (rect, _resp) =
                                ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rect, 4.0, egui::Color32::from_gray(50));
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "🎯\n(Preset Default)",
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(150, 150, 180),
                            );
                        }

                        ui.horizontal(|ui| {
                            if ui.small_button("📁 Upload").clicked() {
                                prompt_image(
                                    "choose target image",
                                    self,
                                    |name: String, img: SourceImg, app: &mut VantaMorphApp| {
                                        // Stage the target image
                                        app.gui.staged_target = Some((name, img));
                                        app.gui.staged_target_texture = None; // Will be created on next frame
                                    },
                                );
                            }
                            if self.gui.staged_target.is_some() {
                                if ui.small_button("✕ Clear").clicked() {
                                    self.gui.staged_target = None;
                                    self.gui.staged_target_texture = None;
                                }
                            }
                        });
                    });

                    ui.add_space(12.0);

                    // === START BUTTON ===
                    let has_source = self.gui.staged_source.is_some();
                    let start_text = if has_source {
                        "▶ Start Morph"
                    } else {
                        "▶ Start (need source)"
                    };

                    let start_btn =
                        egui::Button::new(egui::RichText::new(start_text).strong().size(14.0))
                            .fill(if has_source {
                                egui::Color32::from_rgb(60, 140, 80)
                            } else {
                                egui::Color32::from_gray(60)
                            })
                            .min_size(egui::vec2(ui.available_width(), 36.0));

                    if ui.add_enabled(has_source, start_btn).clicked() {
                        if let Some((name, source_img)) = self.gui.staged_source.take() {
                            // Create morph with source and optional custom target
                            let source_img = ensure_reasonable_size(source_img);
                            let mut settings = GenerationSettings::default(Uuid::new_v4(), name);

                            // Use staged target if provided, otherwise use current preset's target
                            if let Some((_target_name, target_img)) = &self.gui.staged_target {
                                let target_img = ensure_reasonable_size(target_img.clone());
                                settings.set_raw_target(target_img);
                            } else if let Some(target) = &self.gui.current_preset_target {
                                settings.set_raw_target(target.clone());
                            }

                            self.gui.show_progress_modal(settings.id);
                            self.gui.saved_config = Some((source_img.clone(), settings.clone()));

                            settings.proximity_importance = (settings.proximity_importance as f32
                                / (settings.sidelen as f32 / 128.0))
                                as i64;

                            self.gui
                                .process_cancelled
                                .store(false, std::sync::atomic::Ordering::Relaxed);

                            let unprocessed = UnprocessedPreset {
                                name: settings.name.clone(),
                                width: source_img.width(),
                                height: source_img.height(),
                                source_img: source_img.into_raw(),
                                target_img: None,
                            };

                            self.resize_textures(
                                device,
                                (settings.sidelen, settings.sidelen),
                                false,
                            );

                            #[cfg(target_arch = "wasm32")]
                            {
                                self.start_job(unprocessed, settings);
                            }

                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                std::thread::spawn({
                                    let mut tx = self.progress_tx.clone();
                                    let cancelled = self.gui.process_cancelled.clone();
                                    move || {
                                        let result = calculate::process(
                                            unprocessed,
                                            settings,
                                            &mut tx,
                                            cancelled,
                                        );
                                        if let Err(err) = result {
                                            tx.send(ProgressMsg::Error(err.to_string())).ok();
                                        }
                                    }
                                });
                            }

                            // Clear staged source (keep target if locked)
                            self.gui.staged_source_texture = None;
                            if !self.gui.lock_target {
                                self.gui.staged_target = None;
                                self.gui.staged_target_texture = None;
                            }
                        }
                    }

                    ui.add_space(8.0);
                    ui.separator();

                    // === Quick Actions ===
                    ui.label(egui::RichText::new("Quick Actions").strong());

                    if ui
                        .button("⚡ Quick Upload")
                        .on_hover_text("Upload and instantly morph")
                        .clicked()
                    {
                        prompt_image(
                            "choose source image",
                            self,
                            |name: String, img: SourceImg, app: &mut VantaMorphApp| {
                                app.gui.pending_auto_morph = Some((name, img));
                            },
                        );
                    }

                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("💡 Tip: Drag & drop images!")
                            .small()
                            .weak(),
                    );
                });
        }

        // === RIGHT SIDE PANEL (Pro Mode) ===
        if self.gui.ui_mode == UiMode::Pro && self.gui.show_right_panel {
            egui::SidePanel::right("right_panel")
                .default_width(250.0)
                .min_width(200.0)
                .resizable(true)
                .frame(egui::Frame::group(&ctx.style()).inner_margin(egui::Margin::same(12)))
                .show(ctx, |ui| {
                    // Tab selector
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.gui.right_panel_tab,
                            RightPanelTab::Presets,
                            "Presets",
                        );
                        ui.selectable_value(
                            &mut self.gui.right_panel_tab,
                            RightPanelTab::Motion,
                            "Motion",
                        );
                        ui.selectable_value(
                            &mut self.gui.right_panel_tab,
                            RightPanelTab::Quality,
                            "Quality",
                        );
                    });
                    ui.separator();

                    match self.gui.right_panel_tab {
                        RightPanelTab::Presets => {
                            ui.heading("Presets");
                            ui.add_space(4.0);

                            // Grid of preset cards
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                let available_width = ui.available_width();
                                let card_size = 80.0;
                                let cols =
                                    ((available_width / (card_size + 8.0)).floor() as usize).max(2);

                                egui::Grid::new("preset_grid")
                                    .num_columns(cols)
                                    .spacing([8.0, 8.0])
                                    .show(ui, |ui| {
                                        for (i, preset) in
                                            self.gui.presets.clone().into_iter().enumerate()
                                        {
                                            let selected = i == self.gui.current_preset;

                                            let response = ui.allocate_response(
                                                egui::vec2(card_size, card_size + 20.0),
                                                egui::Sense::click(),
                                            );

                                            let rect = response.rect;
                                            let img_rect = egui::Rect::from_min_size(
                                                rect.min,
                                                egui::vec2(card_size, card_size),
                                            );

                                            // Background
                                            let bg_color = if selected {
                                                egui::Color32::from_rgb(60, 100, 140)
                                            } else if response.hovered() {
                                                egui::Color32::from_gray(60)
                                            } else {
                                                egui::Color32::from_gray(40)
                                            };
                                            ui.painter().rect_filled(img_rect, 4.0, bg_color);

                                            // Preset icon/thumbnail placeholder
                                            ui.painter().text(
                                                img_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "🖼",
                                                egui::FontId::proportional(24.0),
                                                egui::Color32::WHITE,
                                            );

                                            // Name below
                                            let name_rect = egui::Rect::from_min_max(
                                                egui::pos2(rect.min.x, img_rect.max.y),
                                                rect.max,
                                            );
                                            let display_name = if preset.inner.name.len() > 10 {
                                                format!("{}…", &preset.inner.name[..8])
                                            } else {
                                                preset.inner.name.clone()
                                            };
                                            ui.painter().text(
                                                name_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                display_name,
                                                egui::FontId::proportional(10.0),
                                                egui::Color32::WHITE,
                                            );

                                            if response.clicked() {
                                                if preset.inner.target_img.is_some() {
                                                    self.gui.pending_preset_process = Some(i);
                                                } else {
                                                    self.change_sim(
                                                        device,
                                                        &rs.queue,
                                                        preset.clone(),
                                                        i,
                                                    );
                                                    self.gui.animate = true;
                                                }
                                                self.gui.current_preset = i;
                                            }

                                            // New row after 'cols' items
                                            if (i + 1) % cols == 0 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                        }

                        RightPanelTab::Motion => {
                            ui.heading("Motion");
                            ui.add_space(8.0);

                            // Duration slider
                            ui.label("Duration (seconds):");
                            ui.add(
                                egui::Slider::new(&mut self.gui.animation_duration, 1.0..=10.0)
                                    .suffix("s"),
                            );

                            ui.add_space(8.0);

                            // Motion style
                            ui.label("Motion Style:");
                            egui::ComboBox::from_id_salt("motion_style")
                                .selected_text(self.gui.motion_style.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.gui.motion_style,
                                        MotionStyle::Linear,
                                        "Linear",
                                    );
                                    ui.selectable_value(
                                        &mut self.gui.motion_style,
                                        MotionStyle::Float,
                                        "Float",
                                    );
                                    ui.selectable_value(
                                        &mut self.gui.motion_style,
                                        MotionStyle::Swirl,
                                        "Swirl",
                                    );
                                    ui.selectable_value(
                                        &mut self.gui.motion_style,
                                        MotionStyle::Dust,
                                        "Dust",
                                    );
                                    ui.selectable_value(
                                        &mut self.gui.motion_style,
                                        MotionStyle::MagnetSnap,
                                        "Magnet Snap",
                                    );
                                });

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Motion sliders
                            ui.label("Swirl:");
                            ui.add(egui::Slider::new(&mut self.gui.swirl_amount, 0.0..=1.0));

                            ui.label("Turbulence:");
                            ui.add(egui::Slider::new(&mut self.gui.turbulence, 0.0..=1.0));

                            ui.label("Snap Strength:");
                            ui.add(egui::Slider::new(&mut self.gui.snap_strength, 0.0..=1.0));

                            ui.label("Dissolve:");
                            ui.add(egui::Slider::new(&mut self.gui.dissolve, 0.0..=1.0));
                        }

                        RightPanelTab::Quality => {
                            ui.heading("Quality");
                            ui.add_space(8.0);

                            // Resolution
                            ui.label("Resolution:");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.gui.resolution, 64, "64");
                                ui.selectable_value(&mut self.gui.resolution, 128, "128");
                                ui.selectable_value(&mut self.gui.resolution, 256, "256");
                                ui.selectable_value(&mut self.gui.resolution, 512, "512");
                            });

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Edge boost
                            ui.checkbox(&mut self.gui.edge_boost, "Edge Boost")
                                .on_hover_text("Enhance edge detection for sharper morphs");

                            // Dithering
                            ui.checkbox(&mut self.gui.dither_enabled, "Dithering");
                            if self.gui.dither_enabled {
                                ui.indent("dither_settings", |ui| {
                                    ui.label("Strength:");
                                    ui.add(egui::Slider::new(
                                        &mut self.gui.dither_strength,
                                        0.0..=1.0,
                                    ));
                                });
                            }

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Compare view
                            ui.label("Compare View:");
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.gui.compare_view,
                                    CompareView::None,
                                    "None",
                                );
                                ui.selectable_value(
                                    &mut self.gui.compare_view,
                                    CompareView::BeforeAfter,
                                    "Before/After",
                                );
                                ui.selectable_value(
                                    &mut self.gui.compare_view,
                                    CompareView::Split,
                                    "Split",
                                );
                            });

                            if self.gui.compare_view == CompareView::Split {
                                ui.label("Split Position:");
                                ui.add(egui::Slider::new(&mut self.gui.split_position, 0.0..=1.0));
                            }
                        }
                    }
                });
        }
        if self.gui.configuring_generation.is_some() {
            Window::new("morphing settings")
                .max_width(screen_width.min(400.0) * 0.8)
                //.max_height(500.0)
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    //ctx.set_zoom_factor((screen_width / 400.0).max(1.0) * baseline_zoom);
                    // ui.set_width((screen_width * 0.9).min(400.0));
                    // ui.set_max_height(500.0);
                    let max_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(max_w, 0.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.set_max_width(max_w);
                            // ui.add(egui::Label::new(
                            //     egui::RichText::new("obamification settings")
                            //         .heading()
                            //         .strong(),
                            // ));
                            // ui.separator();
                            ui.allocate_ui_with_layout(
                                egui::vec2(max_w, 0.0),
                                egui::Layout::left_to_right(egui::Align::Center)
                                    .with_main_wrap(true),
                                |ui| {
                                    ui.label("name:");
                                    if let Some((_, settings, _)) =
                                        self.gui.configuring_generation.as_mut()
                                    {
                                        ui.text_edit_singleline(&mut settings.name);
                                    }
                                },
                            );

                            ui.separator();

                            let mut change_source = false;
                            let mut change_target = false;

                            ui.allocate_ui_with_layout(
                                egui::vec2(max_w, 0.0),
                                egui::Layout::left_to_right(egui::Align::Center)
                                    .with_main_wrap(true)
                                    .with_main_justify(true),
                                |ui| {
                                    ui.set_max_width(max_w);
                                    if let Some((source_img, settings, cache)) =
                                        self.gui.configuring_generation.as_mut()
                                    {
                                        change_source = image_crop_gui(
                                            "source",
                                            ui,
                                            source_img,
                                            &mut settings.source_crop_scale,
                                            &mut cache.source_preview,
                                        );
                                        if is_landscape {
                                            // ./arrow-right.svg
                                            ui.vertical(|ui| {
                                                image_overlap_preview(
                                                    "overlap preview",
                                                    ui,
                                                    settings,
                                                    cache,
                                                    source_img,
                                                    &settings.get_raw_target(),
                                                    0.5,
                                                );

                                                ui.add(
                                                    egui::Image::new(egui::include_image!(
                                                        "./arrow-right.svg"
                                                    ))
                                                    .max_size(egui::vec2(50.0, 50.0)),
                                                );
                                            });
                                        }

                                        change_target = image_crop_gui(
                                            "target",
                                            ui,
                                            &settings.get_raw_target(),
                                            &mut settings.target_crop_scale,
                                            &mut cache.target_preview,
                                        );
                                    }
                                },
                            );

                            if change_source {
                                prompt_image(
                                    "choose source image",
                                    self,
                                    |_, mut img: SourceImg, app: &mut VantaMorphApp| {
                                        img = ensure_reasonable_size(img);
                                        if let Some((src, _, cache)) =
                                            &mut app.gui.configuring_generation
                                        {
                                            *src = img;
                                            cache.source_preview = None;
                                        }
                                    },
                                );
                            } else if change_target {
                                prompt_image(
                                    "choose custom target image",
                                    self,
                                    |_, mut img: SourceImg, app: &mut VantaMorphApp| {
                                        img = ensure_reasonable_size(img);
                                        if let Some((_, settings, cache)) =
                                            &mut app.gui.configuring_generation
                                        {
                                            settings.set_raw_target(img);
                                            cache.target_preview = None;
                                        }
                                    },
                                );
                            }

                            ui.separator();

                            if let Some((_img, settings, _)) =
                                self.gui.configuring_generation.as_mut()
                            {
                                egui::CollapsingHeader::new("advanced settings")
                                    .default_open(false)
                                    .show(ui, |ui| {
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(max_w, 0.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                let slider_w = ui.available_width().min(260.0);
                                                ui.add_sized(
                                                    [slider_w, 20.0],
                                                    egui::Slider::new(
                                                        &mut settings.sidelen,
                                                        64..=256,
                                                    )
                                                    .text("resolution"),
                                                );

                                                let slider_w = ui.available_width().min(260.0);
                                                ui.add_sized(
                                                    [slider_w, 20.0],
                                                    egui::Slider::new(
                                                        &mut settings.proximity_importance,
                                                        0..=50,
                                                    )
                                                    .text("proximity importance"),
                                                );

                                                let mut algorithm = match settings.algorithm {
                                                    calculate::util::Algorithm::Optimal => {
                                                        "optimal algorithm"
                                                    }
                                                    calculate::util::Algorithm::Genetic => {
                                                        "fast algorithm"
                                                    }
                                                };

                                                egui::ComboBox::from_id_salt("algorithm_select")
                                                    .selected_text(algorithm)
                                                    .show_ui(ui, |ui| {
                                                        if ui.button("optimal algorithm").clicked()
                                                        {
                                                            algorithm = "optimal algorithm";
                                                            settings.algorithm =
                                                                calculate::util::Algorithm::Optimal;
                                                        }
                                                        if ui.button("fast algorithm").clicked() {
                                                            algorithm = "fast algorithm";
                                                            settings.algorithm =
                                                                calculate::util::Algorithm::Genetic;
                                                        }
                                                    });
                                            },
                                        );
                                    });
                            }
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add(egui::Button::new(egui::RichText::new("start!").strong()))
                                    .clicked()
                                {
                                    if let Some((img, mut settings, _)) =
                                        self.gui.configuring_generation.take()
                                    {
                                        self.gui.show_progress_modal(settings.id);
                                        self.gui.saved_config =
                                            Some((img.clone(), settings.clone()));
                                        //self.gui.currently_processing = Some(path.clone());
                                        //self.change_sim(device, path.clone(), false);

                                        // adjust for consistency across resolutions
                                        settings.proximity_importance =
                                            (settings.proximity_importance as f32
                                                / (settings.sidelen as f32 / 128.0))
                                                as i64;

                                        self.gui
                                            .process_cancelled
                                            .store(false, std::sync::atomic::Ordering::Relaxed);

                                        let unprocessed = UnprocessedPreset {
                                            name: settings.name.clone(),
                                            width: img.width(),
                                            height: img.height(),
                                            source_img: img.into_raw(),
                                            target_img: None,
                                        };

                                        self.resize_textures(
                                            device,
                                            (settings.sidelen, settings.sidelen),
                                            false,
                                        );

                                        #[cfg(target_arch = "wasm32")]
                                        {
                                            self.start_job(unprocessed, settings);
                                        }

                                        #[cfg(not(target_arch = "wasm32"))]
                                        {
                                            std::thread::spawn({
                                                let mut tx = self.progress_tx.clone();
                                                let cancelled = self.gui.process_cancelled.clone();
                                                move || {
                                                    let result = calculate::process(
                                                        unprocessed,
                                                        settings,
                                                        &mut tx,
                                                        cancelled,
                                                    );
                                                    if let Err(err) = result {
                                                        tx.send(ProgressMsg::Error(
                                                            err.to_string(),
                                                        ))
                                                        .ok();
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                                if ui.button("cancel").clicked() {
                                    self.gui.configuring_generation = None;
                                    #[cfg(target_arch = "wasm32")]
                                    show_icons();
                                }
                            });
                        },
                    );
                });
        }

        if let Some(progress_id) = self.gui.show_progress_modal {
            Window::new(progress_id.to_string())
                .title_bar(false)
                .collapsible(false)
                .movable(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_BOTTOM, (0.0, 0.0))
                .show(ctx, |ui| {
                    let processing_label_message = "processing...";
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width().min(400.0));
                        while let Some(msg) = self.get_latest_msg() {
                            match msg {
                                ProgressMsg::Done(new_preset) => {
                                    self.preview_image = None;
                                    self.resize_textures(
                                        device,
                                        (DEFAULT_RESOLUTION, DEFAULT_RESOLUTION),
                                        false,
                                    );

                                    // Replace existing preset or add new one
                                    let preset_index =
                                        if let Some(idx) = self.gui.replacing_preset_index.take() {
                                            self.gui.presets[idx] = new_preset.clone();
                                            idx
                                        } else {
                                            self.gui.presets.push(new_preset.clone());
                                            self.gui.presets.len() - 1
                                        };

                                    self.change_sim(device, &rs.queue, new_preset, preset_index);
                                    self.gui.animate = true;
                                    self.gui.has_morphed_once = true;
                                    self.gui.hide_progress_modal();
                                    ui.close();
                                    break;
                                }
                                ProgressMsg::Progress(p) => {
                                    self.gui.last_progress = p;
                                }
                                ProgressMsg::Error(err) => {
                                    ui.label(format!("error: {}", err));
                                    if ui.button("close").clicked() {
                                        ui.close();
                                    }
                                }
                                ProgressMsg::UpdatePreview {
                                    width,
                                    height,
                                    data,
                                } => {
                                    let image = image::ImageBuffer::from_vec(width, height, data);
                                    self.preview_image = image;
                                }
                                ProgressMsg::Cancelled => {
                                    self.preview_image = None;
                                    self.resize_textures(
                                        device,
                                        (DEFAULT_RESOLUTION, DEFAULT_RESOLUTION),
                                        false,
                                    );
                                    self.gui.hide_progress_modal();
                                    ui.close();
                                }
                                ProgressMsg::UpdateAssignments(assignments) => {
                                    self.sim.set_assignments(assignments, self.size.0)
                                }
                            }
                        }

                        if self.gui.process_cancelled.load(Ordering::Relaxed) {
                            ui.label("cancelling...");
                        } else if self.gui.last_progress == 0.0 {
                            ui.label("preparing...");
                        } else {
                            ui.label(processing_label_message);
                        }
                        ui.add(egui::ProgressBar::new(self.gui.last_progress).show_percentage());

                        ui.horizontal(|ui| {
                            if ui.button("cancel").clicked() {
                                #[cfg(target_arch = "wasm32")]
                                {
                                    if let Some(w) = &self.worker {
                                        w.terminate();
                                    }
                                    self.worker = None;
                                    self.preview_image = None;
                                    self.resize_textures(
                                        device,
                                        (DEFAULT_RESOLUTION, DEFAULT_RESOLUTION),
                                        false,
                                    );
                                    self.gui.hide_progress_modal();
                                    ui.close();
                                }
                                self.gui.process_cancelled.store(true, Ordering::Relaxed);
                                self.gui.last_progress = 0.0;
                            }
                        })
                    });
                });
        } else if !self.gif_recorder.not_recording() {
            Modal::new(format!("recording_progress_{}", self.gif_recorder.id).into()).show(
                ctx,
                |ui| {
                    match self.gif_recorder.status.clone() {
                        GifStatus::Recording => {
                            ui.label("recording gif...");
                            if ui.button("cancel").clicked() {
                                self.stop_recording_gif(device, &rs.queue);
                                self.gui.animate = false;
                            }
                        }

                        GifStatus::Error(err) => {
                            ui.label(format!("Error: {}", err));
                            ui.horizontal(|ui| {
                                if ui.button("close").clicked() {
                                    self.stop_recording_gif(device, &rs.queue);
                                }
                            });
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        GifStatus::Complete(path) => {
                            ui.label("gif saved!");
                            ui.horizontal(|ui| {
                                if ui.button("open file").clicked() {
                                    opener::reveal(path).ok();
                                }
                                if ui.button("close").clicked() {
                                    self.stop_recording_gif(device, &rs.queue);
                                }
                            });
                        }
                        #[cfg(target_arch = "wasm32")]
                        GifStatus::Complete => {
                            // save opens dialog automatically
                            self.stop_recording_gif(device, &rs.queue);
                        }
                        GifStatus::None => unreachable!(),
                    }
                },
            );
        }
        if let Some(err) = &self.gui.error_message {
            let mut close = false;
            Window::new("error")
                .collapsible(false)
                .movable(true)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(err);
                    if ui.button("close").clicked() {
                        close = true;
                    }
                });
            if close {
                self.gui.hide_error();
            }
        }

        // === BOTTOM PLAYBACK PANEL ===
        egui::TopBottomPanel::bottom("playback_panel")
            .frame(egui::Frame::group(&ctx.style()).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ctx, |ui| {
                // Two rows: Timeline on top, controls below
                ui.vertical(|ui| {
                    // === Timeline Scrubber Row ===
                    ui.horizontal(|ui| {
                        // Time display (current position)
                        let progress_pct = (self.gui.timeline_position * 100.0) as u32;
                        ui.label(
                            egui::RichText::new(format!("{}%", progress_pct))
                                .monospace()
                                .size(12.0),
                        );

                        // Timeline slider
                        let slider_response = ui.add(
                            egui::Slider::new(&mut self.gui.timeline_position, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );

                        // Handle scrubbing interaction
                        if slider_response.dragged() {
                            self.gui.scrubbing = true;
                            // TODO: Seek to position when timeline support is added
                        } else if self.gui.scrubbing && slider_response.drag_stopped() {
                            self.gui.scrubbing = false;
                        }

                        // Duration display
                        ui.label(
                            egui::RichText::new(format!("{:.1}s", self.gui.animation_duration))
                                .monospace()
                                .size(12.0),
                        );
                    });

                    ui.add_space(4.0);

                    // === Playback Controls Row ===
                    ui.horizontal(|ui| {
                        // Left controls: Play/Pause, Reverse, Loop, Speed

                        // Play/Pause button (larger)
                        let play_icon = if self.gui.animate { "⏸" } else { "▶" };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(play_icon).size(20.0))
                                    .min_size(egui::vec2(36.0, 36.0)),
                            )
                            .on_hover_text(if self.gui.animate {
                                "Pause (Space)"
                            } else {
                                "Play (Space)"
                            })
                            .clicked()
                        {
                            self.gui.animate = !self.gui.animate;
                            if self.gui.animate {
                                self.sim.prepare_play(&mut self.seeds, self.reverse);
                            }
                        }

                        // Reverse button
                        let reverse_icon = if self.reverse { "⏪" } else { "⏩" };
                        let reverse_color = if self.reverse {
                            Color32::from_rgb(100, 180, 255)
                        } else {
                            Color32::GRAY
                        };
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(reverse_icon)
                                    .size(16.0)
                                    .color(reverse_color),
                            ))
                            .on_hover_text(format!(
                                "Direction: {} (R)",
                                if self.reverse { "Reverse" } else { "Forward" }
                            ))
                            .clicked()
                        {
                            self.reverse = !self.reverse;
                            self.sim.prepare_play(&mut self.seeds, self.reverse);
                            self.gui.animate = true;
                        }

                        // Loop toggle
                        let loop_color = if self.gui.loop_playback {
                            Color32::from_rgb(100, 200, 100)
                        } else {
                            Color32::GRAY
                        };
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("🔁").size(14.0).color(loop_color),
                            ))
                            .on_hover_text(format!(
                                "Loop: {} (L)",
                                if self.gui.loop_playback { "On" } else { "Off" }
                            ))
                            .clicked()
                        {
                            self.gui.loop_playback = !self.gui.loop_playback;
                        }

                        ui.separator();

                        // Speed selector
                        ui.label("Speed:");
                        egui::ComboBox::from_id_salt("speed_select")
                            .width(60.0)
                            .selected_text(self.gui.playback_speed.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.gui.playback_speed,
                                    PlaybackSpeed::Quarter,
                                    "0.25×",
                                );
                                ui.selectable_value(
                                    &mut self.gui.playback_speed,
                                    PlaybackSpeed::Half,
                                    "0.5×",
                                );
                                ui.selectable_value(
                                    &mut self.gui.playback_speed,
                                    PlaybackSpeed::Normal,
                                    "1×",
                                );
                                ui.selectable_value(
                                    &mut self.gui.playback_speed,
                                    PlaybackSpeed::Double,
                                    "2×",
                                );
                            });

                        // Spacer
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Simple/Pro toggle (right side)
                            let mode_text = match self.gui.ui_mode {
                                UiMode::Simple => "🔧 Pro",
                                UiMode::Pro => "✨ Simple",
                            };
                            if ui
                                .button(mode_text)
                                .on_hover_text("Toggle Simple/Pro mode")
                                .clicked()
                            {
                                self.gui.ui_mode = match self.gui.ui_mode {
                                    UiMode::Simple => UiMode::Pro,
                                    UiMode::Pro => UiMode::Simple,
                                };
                            }

                            ui.separator();

                            // Drawing mode button (desktop only)
                            #[cfg(not(target_arch = "wasm32"))]
                            if ui.button("✏️ Draw").on_hover_text("Drawing mode").clicked() {
                                self.gui.mode = GuiMode::Draw;
                                self.init_canvas(device, &rs.queue);
                            }
                        });
                    });
                });
            });

        // === HANDLE FILE DROPS (egui native) ===
        #[cfg(target_arch = "wasm32")]
        {
            // Hide drop overlay when drag ends
            ctx.input(|i| {
                // Hide overlay when no longer hovering with files
                if i.raw.hovered_files.is_empty() {
                    if let Some(window) = web_sys::window() {
                        if let Some(document) = window.document() {
                            if let Some(overlay) = document.get_element_by_id("drop-overlay") {
                                let _ = overlay.set_attribute("style", 
                                    "position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,0.8);display:none;justify-content:center;align-items:center;z-index:9999;pointer-events:none;");
                            }
                        }
                    }
                } else {
                    // Show overlay when hovering with files
                    if let Some(window) = web_sys::window() {
                        if let Some(document) = window.document() {
                            if let Some(overlay) = document.get_element_by_id("drop-overlay") {
                                let _ = overlay.set_attribute("style", 
                                    "position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,0.8);display:flex;justify-content:center;align-items:center;z-index:9999;pointer-events:none;");
                            }
                        }
                    }
                }
            });

            // Use egui's built-in file drop handling
            if self.gui.pending_auto_morph.is_none()
                && self.gui.show_progress_modal.is_none()
                && self.gui.configuring_generation.is_none()
            {
                ctx.input(|i| {
                    if !i.raw.dropped_files.is_empty() {
                        for file in &i.raw.dropped_files {
                            // Try to get the file bytes
                            if let Some(bytes) = &file.bytes {
                                let name = file.name.clone();
                                let name = if name.is_empty() {
                                    "dropped_image".to_string()
                                } else {
                                    // Remove file extension from name
                                    name.rsplit_once('.')
                                        .map(|(n, _)| n.to_string())
                                        .unwrap_or(name)
                                };

                                // Try to load as image
                                match image::load_from_memory(bytes) {
                                    Ok(img) => {
                                        self.gui.pending_auto_morph = Some((name, img.to_rgb8()));

                                        // Hide the drop hint after successful drop
                                        if let Some(window) = web_sys::window() {
                                            if let Some(document) = window.document() {
                                                if let Some(hint) =
                                                    document.get_element_by_id("drop-hint")
                                                {
                                                    let _ = hint
                                                        .set_attribute("style", "display:none;");
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        web_sys::console::error_1(
                                            &format!("Failed to load dropped image: {}", e).into(),
                                        );
                                    }
                                }
                                break; // Only handle first file
                            }
                        }
                    }
                });
            }
        }

        // === HANDLE KEYBOARD SHORTCUTS ===
        ctx.input(|i| {
            // Space = Play/Pause
            if i.key_pressed(egui::Key::Space) && self.gui.show_progress_modal.is_none() {
                self.gui.animate = !self.gui.animate;
                if self.gui.animate {
                    self.sim.prepare_play(&mut self.seeds, self.reverse);
                }
            }
            // R = Reverse
            if i.key_pressed(egui::Key::R) && self.gui.show_progress_modal.is_none() {
                self.reverse = !self.reverse;
                self.sim.prepare_play(&mut self.seeds, self.reverse);
                self.gui.animate = true;
            }
            // L = Loop toggle
            if i.key_pressed(egui::Key::L) && self.gui.show_progress_modal.is_none() {
                self.gui.loop_playback = !self.gui.loop_playback;
            }
            // P = Toggle panels (Pro mode)
            if i.key_pressed(egui::Key::P) && self.gui.show_progress_modal.is_none() {
                if self.gui.ui_mode == UiMode::Pro {
                    // Toggle both panels
                    let both_visible = self.gui.show_left_panel && self.gui.show_right_panel;
                    self.gui.show_left_panel = !both_visible;
                    self.gui.show_right_panel = !both_visible;
                }
            }
            // Tab = Cycle right panel tabs (Pro mode)
            if i.key_pressed(egui::Key::Tab)
                && self.gui.show_progress_modal.is_none()
                && self.gui.ui_mode == UiMode::Pro
            {
                self.gui.right_panel_tab = match self.gui.right_panel_tab {
                    RightPanelTab::Presets => RightPanelTab::Motion,
                    RightPanelTab::Motion => RightPanelTab::Quality,
                    RightPanelTab::Quality => RightPanelTab::Presets,
                };
            }
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                // Main canvas area with overlays
                let panel_rect = ui.available_rect_before_wrap();

                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        if let Some(id) = self.egui_tex_id {
                            let full = ui.available_size();
                            let aspect = self.size.0 as f32 / self.size.1 as f32;
                            let desired = full.x.min(full.y) * egui::vec2(1.0, aspect);
                            ui.add(egui::Image::new((id, desired)).maintain_aspect_ratio(true));

                            #[cfg(not(target_arch = "wasm32"))]
                            if matches!(self.gui.mode, GuiMode::Draw) {
                                self.handle_drawing(ctx, device, &rs.queue, ui, aspect);
                            }
                        } else {
                            ui.colored_label(Color32::LIGHT_RED, "Texture not ready");
                        }
                    },
                );

                // === Canvas Overlays ===
                if self.gui.show_overlays {
                    // Bottom-left overlay: Stats
                    let overlay_margin = 8.0;
                    let stats_pos = egui::pos2(
                        panel_rect.min.x + overlay_margin,
                        panel_rect.max.y - overlay_margin - 50.0,
                    );

                    egui::Area::new("canvas_stats_overlay".into())
                        .fixed_pos(stats_pos)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::popup(&ctx.style())
                                .fill(Color32::from_rgba_unmultiplied(20, 20, 20, 200))
                                .inner_margin(egui::Margin::same(6))
                                .corner_radius(4.0)
                                .show(ui, |ui| {
                                    let particle_count = self.size.0 * self.size.1;
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Particles: {}k",
                                            particle_count / 1000
                                        ))
                                        .small()
                                        .color(Color32::LIGHT_GRAY),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Resolution: {}×{}",
                                            self.size.0, self.size.1
                                        ))
                                        .small()
                                        .color(Color32::LIGHT_GRAY),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Speed: {}",
                                            self.gui.playback_speed.label()
                                        ))
                                        .small()
                                        .color(Color32::LIGHT_GRAY),
                                    );
                                });
                        });
                }

                // Show hint when no morph is active (Simple mode)
                if self.gui.ui_mode == UiMode::Simple && !self.gui.has_morphed_once {
                    let hint_pos = egui::pos2(panel_rect.center().x, panel_rect.max.y - 80.0);

                    egui::Area::new("upload_hint_overlay".into())
                        .fixed_pos(hint_pos)
                        .pivot(egui::Align2::CENTER_CENTER)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::popup(&ctx.style())
                                .fill(Color32::from_rgba_unmultiplied(40, 80, 120, 220))
                                .inner_margin(egui::Margin::same(12))
                                .corner_radius(8.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "📁 Drop an image here or click Upload",
                                        )
                                        .color(Color32::WHITE),
                                    );
                                    ui.label(
                                        egui::RichText::new("to create your first morph!")
                                            .small()
                                            .color(Color32::LIGHT_GRAY),
                                    );
                                });
                        });
                }
            });
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(self.gui.mode, GuiMode::Draw) {
            let number_keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ];

            // DBECEE,383232, 6B5E57,D49976

            let colors = [
                ("black", 0x000000),
                ("a", 0x86d9e3),
                ("b", 0x383232),
                ("c", 0xD49976),
                ("d", 0x793025),
            ];

            for (idx, (_name, color)) in colors.iter().enumerate() {
                if ctx.input(|i| i.key_pressed(number_keys[idx])) {
                    let hex = *color;
                    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
                    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
                    let b = (hex & 0xFF) as f32 / 255.0;
                    let a = 0.5;

                    self.gui.drawing_color = [r, g, b, a];
                }
            }
            // show selected drawing color
            egui::Area::new("drawing_color".into())
                .anchor(egui::Align2::LEFT_TOP, egui::vec2(10.0, 30.0))
                .show(ctx, |ui| {
                    let rect_size = 30.0;
                    let (rect, _resp) = ui.allocate_exact_size(
                        egui::vec2(rect_size, rect_size),
                        egui::Sense::hover(),
                    );
                    let color = egui::Color32::from_rgba_unmultiplied(
                        (self.gui.drawing_color[0] * 255.0) as u8,
                        (self.gui.drawing_color[1] * 255.0) as u8,
                        (self.gui.drawing_color[2] * 255.0) as u8,
                        255,
                    );
                    ui.painter().rect_filled(rect, 15.0, color);
                    if ui.is_rect_visible(rect) {
                        ui.painter().rect_stroke(
                            rect,
                            15.0,
                            (2.0, egui::Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }

                    // Keep the picker visible while hovering either the main swatch or the picker area.
                    let spacing = 10.0;
                    let btn_size = rect_size / 2.0;
                    let gap = 4.0;

                    // Layout picker row next to the swatch, vertically centered.
                    let n_buttons = colors.len() as f32;
                    let picker_width = n_buttons * btn_size + (n_buttons - 1.0).max(0.0) * gap;
                    let picker_min =
                        rect.min + egui::vec2(rect_size + spacing, (rect_size - btn_size) * 0.5);
                    let picker_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(picker_width + rect_size + spacing, rect_size),
                    );

                    // Decide visibility purely from pointer position to avoid z-order flicker.
                    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
                    let show_picker = ui.is_rect_visible(rect)
                        && pointer_pos.is_some_and(|p| rect.contains(p) || picker_rect.contains(p));

                    // Global visibility animation driver
                    let base_t = ui
                        .ctx()
                        .animate_bool(egui::Id::new("color_picker_visible"), show_picker);

                    // Helpers
                    let saturate = |x: f32| x.clamp(0.0, 1.0);
                    let smoothstep = |x: f32| {
                        let x = saturate(x);
                        x * x * (3.0 - 2.0 * x)
                    };

                    // Start position = centered under the main swatch so buttons "emerge" from it
                    let start_pos = egui::pos2(
                        rect.min.x + (rect_size - btn_size) * 0.5,
                        rect.min.y + (rect_size - btn_size) * 0.5,
                    );

                    // Per-button stagger
                    let per_btn_delay = 0.08_f32;
                    // Ensure the last button still reaches t=1 when base_t=1
                    let total_stagger = (n_buttons - 1.0).max(0.0) * per_btn_delay;
                    let denom = (1.0 - total_stagger).max(1e-6);

                    for (idx, (_name, hex)) in colors.iter().enumerate() {
                        let rgba = {
                            let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
                            let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
                            let b = (hex & 0xFF) as f32 / 255.0;
                            let a = DRAWING_ALPHA;
                            [r, g, b, a]
                        };
                        let i = idx as f32;

                        // Staggered progress for each button; normalized so the last also reaches 1.0
                        let raw = (base_t - per_btn_delay * i) / denom;
                        let t_i = smoothstep(raw);

                        // Only draw while animating or visible to avoid early reveal
                        if t_i <= 0.001 {
                            continue;
                        }

                        // Target position to the right of the swatch
                        let end_pos = egui::pos2(picker_min.x + i * (btn_size + gap), picker_min.y);

                        // Interpolate from under the swatch to the target
                        let pos = egui::pos2(
                            egui::lerp(start_pos.x..=end_pos.x, t_i),
                            egui::lerp(start_pos.y..=end_pos.y, t_i),
                        );

                        egui::Area::new(egui::Id::new(format!("color_picker_btn_{idx}")))
                            .fixed_pos(pos)
                            .show(ctx, |ui| {
                                let (btn_rect, btn_resp) = ui.allocate_exact_size(
                                    egui::vec2(btn_size, btn_size),
                                    egui::Sense::click(),
                                );

                                // Fade with the slide
                                let a = (255.0 * t_i) as u8;
                                let color32 = egui::Color32::from_rgba_unmultiplied(
                                    (rgba[0] * 255.0) as u8,
                                    (rgba[1] * 255.0) as u8,
                                    (rgba[2] * 255.0) as u8,
                                    a,
                                );

                                ui.painter().rect_filled(btn_rect, 15.0 / 2.0, color32);
                                if ui.is_rect_visible(btn_rect) {
                                    ui.painter().rect_stroke(
                                        btn_rect,
                                        15.0 / 2.0,
                                        (
                                            2.0,
                                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, a),
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                }

                                if btn_resp.clicked() {
                                    self.gui.drawing_color = rgba;
                                }
                            });
                    }
                });
        }

        // continuous repaint for animation
        ctx.request_repaint();
        self.frame_count += 1;
    }
}

fn prompt_image(
    title: &'static str,
    app: &mut VantaMorphApp,
    callback: impl FnOnce(String, image::RgbImage, &mut VantaMorphApp) + 'static,
) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;
        let app_ptr: *mut VantaMorphApp = app;

        spawn_local(async move {
            if let Some(handle) = rfd::AsyncFileDialog::new()
                .set_title(title)
                .add_filter("image files", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
                .await
            {
                let name = get_default_preset_name(handle.file_name());
                let data = handle.read().await;
                match image::load_from_memory(&data) {
                    Ok(img) => unsafe {
                        if let Some(app) = app_ptr.as_mut() {
                            callback(name, img.to_rgb8(), app);
                        }
                    },
                    Err(e) => unsafe {
                        if let Some(app) = app_ptr.as_mut() {
                            app.gui.show_error(format!("failed to load image: {}", e));
                        }
                    },
                }
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(file) = rfd::FileDialog::new()
            .set_title(title)
            .add_filter("image files", &["png", "jpg", "jpeg", "webp"])
            .pick_file()
        {
            let name =
                get_default_preset_name(file.file_name().unwrap().to_string_lossy().to_string());

            match image::open(file) {
                Ok(img) => callback(name, img.to_rgb8(), app),
                Err(e) => app.gui.show_error(format!("failed to load image: {}", e)),
            }
        }
    }
}

fn ensure_reasonable_size(img: SourceImg) -> SourceImg {
    let max_side = 512;
    let (w, h) = img.dimensions();
    if w <= max_side && h <= max_side {
        return img;
    }
    let scale = (max_side as f32 / w as f32).min(max_side as f32 / h as f32);
    let new_w = (w as f32 * scale).round() as u32;
    let new_h = (h as f32 * scale).round() as u32;

    image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3)
}

fn image_overlap_preview(
    arg: &str,
    ui: &mut egui::Ui,
    settings: &GenerationSettings,
    cache: &mut GuiImageCache,
    source_img: &SourceImg,
    get_raw_target: &SourceImg,
    blend: f32,
) {
    let tex = if cache.overlap_preview.is_none()
        || cache.source_preview.is_none()
        || cache.target_preview.is_none()
    {
        let src_img = settings.source_crop_scale.apply(source_img, 64);
        let tgt_img = settings.target_crop_scale.apply(get_raw_target, 64);
        let blended = blend_rgb_images(&src_img, &tgt_img, blend);
        let p = ui.ctx().load_texture(
            arg,
            egui::ColorImage::from_rgb([64, 64], blended.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        cache.overlap_preview = Some(p.clone());
        p
    } else {
        cache.overlap_preview.as_ref().unwrap().clone()
    };
    ui.add(egui::Image::from_texture(&tex));
}

fn image_crop_gui(
    name: &'static str,
    ui: &mut egui::Ui,
    img: &SourceImg,
    crop_scale: &mut CropScale,
    cache: &mut Option<TextureHandle>,
) -> bool {
    let mut open_file_dialog = false;
    ui.vertical(|ui| {
        let tex = match &cache {
            None => {
                let p = ui.ctx().load_texture(
                    name,
                    egui::ColorImage::from_rgb([128, 128], crop_scale.apply(img, 128).as_raw()),
                    egui::TextureOptions::LINEAR,
                );
                *cache = Some(p.clone());
                p
            }
            Some(t) => t.clone(),
        };
        ui.add(egui::Image::from_texture(&tex));
        if ui.button(format!("change {name} image")).clicked() {
            open_file_dialog = true;
        }
        // crop sliders
        ui.vertical(|ui| {
            let values = *crop_scale;
            let slider_w = ui.available_width().min(260.0);

            ui.add_sized(
                [slider_w, 20.0],
                egui::Slider::new(&mut crop_scale.scale, 1.0..=5.0)
                    .show_value(false)
                    .text("zoom"),
            );
            ui.add_sized(
                [slider_w, 20.0],
                egui::Slider::new(&mut crop_scale.x, -1.0..=1.0)
                    .show_value(false)
                    .text("x-off."),
            );
            ui.add_sized(
                [slider_w, 20.0],
                egui::Slider::new(&mut crop_scale.y, -1.0..=1.0)
                    .show_value(false)
                    .text("y-off."),
            );

            if values != *crop_scale {
                *cache = None; // force reload
            }
        });
    });

    open_file_dialog
}

fn get_default_preset_name(mut n: String) -> String {
    let mut name = {
        if let Some(dot) = n.rfind('.') {
            if dot > 0 {
                n.truncate(dot);
            }
        }
        if n.is_empty() {
            "untitled".to_owned()
        } else {
            n
        }
    };
    if name.chars().count() > 20 {
        name = name.chars().take(20).collect();
    }
    name
}

// fn blend_rgb_images(a: &image::RgbImage, b: &image::RgbImage, alpha: f32) -> image::RgbImage {
//     assert_eq!(
//         a.dimensions(),
//         b.dimensions(),
//         "Images must have same dimensions"
//     );
//     let (w, h) = a.dimensions();
//     let alpha = alpha.clamp(0.0, 1.0);
//     let inv = 1.0 - alpha;
//     let mut out = image::RgbImage::new(w, h);
//     for y in 0..h {
//         for x in 0..w {
//             let pa = a.get_pixel(x, y);
//             let pb = b.get_pixel(x, y);
//             let r = (pa[0] as f32 * inv + pb[0] as f32 * alpha).round() as u8;
//             let g = (pa[1] as f32 * inv + pb[1] as f32 * alpha).round() as u8;
//             let bch = (pa[2] as f32 * inv + pb[2] as f32 * alpha).round() as u8;
//             out.put_pixel(x, y, image::Rgb([r, g, bch]));
//         }
//     }
//     out
// }

pub fn blend_rgb_images(a: &SourceImg, b: &SourceImg, alpha: f32) -> SourceImg {
    assert_eq!(
        a.dimensions(),
        b.dimensions(),
        "Images must have same dimensions"
    );

    let (w, h) = a.dimensions();
    let k = alpha.clamp(0.0, 1.0);
    let sigma = 1.5;
    let a_blur = imageops::blur(a, sigma);
    let b_blur = imageops::blur(b, sigma);

    let mut out = SourceImg::new(w, h);

    for y in 0..h {
        for x in 0..w {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            let ga = a_blur.get_pixel(x, y);
            let gb = b_blur.get_pixel(x, y);

            let l0 = 0.5 * (ga[0] as f32 + gb[0] as f32);
            let l1 = 0.5 * (ga[1] as f32 + gb[1] as f32);
            let l2 = 0.5 * (ga[2] as f32 + gb[2] as f32);

            let ha0 = pa[0] as f32 - ga[0] as f32;
            let ha1 = pa[1] as f32 - ga[1] as f32;
            let ha2 = pa[2] as f32 - ga[2] as f32;

            let hb0 = pb[0] as f32 - gb[0] as f32;
            let hb1 = pb[1] as f32 - gb[1] as f32;
            let hb2 = pb[2] as f32 - gb[2] as f32;

            let r0 = (l0 + k * (ha0 + hb0)).clamp(0.0, 255.0).round() as u8;
            let r1 = (l1 + k * (ha1 + hb1)).clamp(0.0, 255.0).round() as u8;
            let r2 = (l2 + k * (ha2 + hb2)).clamp(0.0, 255.0).round() as u8;

            out.put_pixel(x, y, image::Rgb([r0, r1, r2]));
        }
    }

    out
}
