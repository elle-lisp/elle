//! WindowState — winit + glutin + egui-glow, single-threaded.

use crate::ui::{Interactions, WidgetState};
use std::num::NonZeroU32;
use std::os::unix::io::RawFd;
use std::sync::Arc;

use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, Version,
};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// All windowing state. Not Send — lives on the Elle thread.
pub struct WindowState {
    pub event_loop: Option<EventLoop<()>>,
    pub window: Option<Window>,
    pub gl_surface: Option<Surface<WindowSurface>>,
    pub gl_context: Option<PossiblyCurrentContext>,
    pub gl: Option<Arc<glow::Context>>,
    pub egui_ctx: egui::Context,
    pub egui_winit: Option<egui_winit::State>,
    pub painter: Option<egui_glow::Painter>,
    pub display_fd: RawFd,
    pub widget_state: WidgetState,
    pub close_requested: bool,
}

/// Temporary handler for pump_app_events. Collects window events.
struct PumpHandler<'a> {
    state: &'a mut WindowState,
    events: Vec<WindowEvent>,
}

impl ApplicationHandler for PumpHandler<'_> {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Feed to egui-winit for input processing
        if let Some(ref mut egui_winit) = self.state.egui_winit {
            if let Some(ref window) = self.state.window {
                let _ = egui_winit.on_window_event(window, &event);
            }
        }

        match &event {
            WindowEvent::CloseRequested => {
                self.state.close_requested = true;
            }
            WindowEvent::Resized(_) | WindowEvent::RedrawRequested => {}
            _ => {}
        }

        self.events.push(event);
    }
}

impl WindowState {
    pub fn new(title: &str, width: f64, height: f64) -> Result<Self, String> {
        let event_loop = EventLoop::builder()
            .build()
            .map_err(|e| format!("failed to create event loop: {}", e))?;

        let window_attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(width, height));

        let config_template = ConfigTemplateBuilder::new().with_alpha_size(8);

        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(window_attrs))
            .build(&event_loop, config_template, |configs| {
                configs
                    .reduce(|a, b| {
                        if a.num_samples() > b.num_samples() {
                            a
                        } else {
                            b
                        }
                    })
                    .unwrap()
            })
            .map_err(|e| format!("failed to build display: {}", e))?;

        let window = window.ok_or("failed to create window")?;
        let raw_window_handle = window
            .window_handle()
            .map_err(|e| format!("window handle error: {}", e))?;

        let gl_display = gl_config.display();
        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 2))))
            .build(Some(raw_window_handle.as_raw()));

        let gl_context = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .map_err(|e| format!("failed to create GL context: {}", e))?
        };

        let size = window.inner_size();
        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle.as_raw(),
            NonZeroU32::new(size.width.max(1)).unwrap(),
            NonZeroU32::new(size.height.max(1)).unwrap(),
        );

        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attrs)
                .map_err(|e| format!("failed to create surface: {}", e))?
        };

        let gl_context = gl_context
            .make_current(&gl_surface)
            .map_err(|e| format!("failed to make context current: {}", e))?;

        let gl = unsafe {
            Arc::new(glow::Context::from_loader_function_cstr(|s| {
                gl_display.get_proc_address(s)
            }))
        };

        let egui_ctx = egui::Context::default();

        let egui_winit = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        let painter = egui_glow::Painter::new(Arc::clone(&gl), "", None, false)
            .map_err(|e| format!("failed to create painter: {}", e))?;

        // Extract display fd
        let display_fd = get_display_fd(&window)?;

        Ok(WindowState {
            event_loop: Some(event_loop),
            window: Some(window),
            gl_surface: Some(gl_surface),
            gl_context: Some(gl_context),
            gl: Some(gl),
            egui_ctx,
            egui_winit: Some(egui_winit),
            painter: Some(painter),
            display_fd,
            widget_state: WidgetState::new(),
            close_requested: false,
        })
    }

    /// Non-blocking pump of winit events.
    pub fn pump_events(&mut self) {
        if let Some(mut event_loop) = self.event_loop.take() {
            let mut handler = PumpHandler {
                state: self,
                events: Vec::new(),
            };
            use winit::platform::pump_events::EventLoopExtPumpEvents;
            let _ = event_loop.pump_app_events(Some(std::time::Duration::ZERO), &mut handler);
            self.event_loop = Some(event_loop);
        }
    }

    /// Combined: run egui ctx with nodes, paint, return.
    pub fn frame_with_tree(&mut self, nodes: &[crate::ui::UiNode], ix: &mut Interactions) {
        let window = self.window.as_ref().unwrap();
        let gl_surface = self.gl_surface.as_ref().unwrap();
        let gl_context = self.gl_context.as_ref().unwrap();
        let painter = self.painter.as_mut().unwrap();
        let egui_winit = self.egui_winit.as_mut().unwrap();

        let raw_input = egui_winit.take_egui_input(window);
        let size = window.inner_size();
        ix.width = size.width as f32;
        ix.height = size.height as f32;
        ix.closed = self.close_requested;

        let widget_state = &mut self.widget_state;
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::ui::render_tree(ui, nodes, widget_state, ix);
            });
        });

        egui_winit.handle_platform_output(window, full_output.platform_output);

        let pixels_per_point = self.egui_ctx.pixels_per_point();
        painter.paint_and_update_textures(
            [size.width, size.height],
            pixels_per_point,
            &self
                .egui_ctx
                .tessellate(full_output.shapes, pixels_per_point),
            &full_output.textures_delta,
        );

        gl_surface.swap_buffers(gl_context).unwrap();
    }
}

/// Resolve a C function by name from a shared library via dlsym.
unsafe fn resolve_fn(lib: &str, name: &str) -> Result<*mut std::ffi::c_void, String> {
    let lib_cstr = std::ffi::CString::new(lib).map_err(|_| format!("invalid lib name: {}", lib))?;
    let name_cstr =
        std::ffi::CString::new(name).map_err(|_| format!("invalid symbol name: {}", name))?;
    let handle = libc::dlopen(lib_cstr.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL);
    if handle.is_null() {
        return Err(format!(
            "dlopen({}): {}",
            lib,
            std::ffi::CStr::from_ptr(libc::dlerror()).to_string_lossy()
        ));
    }
    let sym = libc::dlsym(handle, name_cstr.as_ptr());
    if sym.is_null() {
        return Err(format!("dlsym({}): symbol not found", name));
    }
    Ok(sym)
}

/// Extract the X11/Wayland display connection fd.
fn get_display_fd(window: &Window) -> Result<RawFd, String> {
    use raw_window_handle::RawDisplayHandle;

    let handle = window
        .display_handle()
        .map_err(|e| format!("display handle error: {}", e))?;

    match handle.as_raw() {
        RawDisplayHandle::Xlib(h) => {
            let display = h.display.ok_or("Xlib display is null")?;
            type XConnectionNumberFn =
                unsafe extern "C" fn(*mut std::ffi::c_void) -> std::ffi::c_int;
            let func: XConnectionNumberFn =
                unsafe { std::mem::transmute(resolve_fn("libX11.so.6", "XConnectionNumber")?) };
            Ok(unsafe { func(display.as_ptr()) })
        }
        RawDisplayHandle::Xcb(h) => {
            let conn = h.connection.ok_or("XCB connection is null")?;
            type XcbGetFdFn = unsafe extern "C" fn(*mut std::ffi::c_void) -> std::ffi::c_int;
            let func: XcbGetFdFn = unsafe {
                std::mem::transmute(resolve_fn("libxcb.so.1", "xcb_get_file_descriptor")?)
            };
            Ok(unsafe { func(conn.as_ptr()) })
        }
        RawDisplayHandle::Wayland(h) => {
            type WlDisplayGetFdFn = unsafe extern "C" fn(*mut std::ffi::c_void) -> std::ffi::c_int;
            let func: WlDisplayGetFdFn = unsafe {
                std::mem::transmute(resolve_fn("libwayland-client.so.0", "wl_display_get_fd")?)
            };
            Ok(unsafe { func(h.display.as_ptr()) })
        }
        _ => Err("unsupported display backend (not X11 or Wayland)".into()),
    }
}
