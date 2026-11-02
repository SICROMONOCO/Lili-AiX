//! Experiment only: reparent the exact Tauri runtime's webview above a GTK GLArea.
//! Software decoding deliberately isolates composition from Linux display/decoder interop.

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anilili_mpv::{Mpv, RenderContext};
use gtk::prelude::*;
use libloading::Library;
use tauri::{Manager, State};

struct Probe(Arc<Mpv>);

#[tauri::command(async)]
fn probe_control(app: tauri::AppHandle, state: State<'_, Probe>, action: String) -> Result<(), String> {
    let mpv = &state.inner().0;
    match action.as_str() {
        "pause" => mpv.command(&["cycle", "pause"]),
        "seek" => mpv.command(&["seek", "10", "relative"]),
        "panel-on" => mpv.set_property("video-margin-ratio-right", "0.25"),
        "panel-off" => mpv.set_property("video-margin-ratio-right", "0"),
        "fullscreen" => {
            let window = app.get_webview_window("main").ok_or("no window")?;
            window.set_fullscreen(!window.is_fullscreen().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            return Ok(());
        }
        _ => return Err("unknown probe action".into()),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn probe_status(state: State<'_, Probe>) -> serde_json::Value {
    let mpv = &state.inner().0;
    serde_json::json!({
        "position": mpv.get_double("time-pos").ok(),
        "duration": mpv.get_double("duration").ok(),
        "pause": mpv.get_string("pause").ok(),
        "decoder": mpv.get_string("hwdec-current").ok(),
        "mpv": mpv.get_string("mpv-version").ok(),
    })
}

// The libraries and resolver remain alive until after the render context is freed.
struct Resolver(Vec<Library>);

impl Resolver {
    fn new() -> Result<Self, String> {
        // SAFETY: load system OpenGL dispatch libraries, retained for all resolved pointers.
        let libraries: Vec<_> = ["libEGL.so.1", "libGL.so.1"].iter().filter_map(|name| unsafe { Library::new(name).ok() }).collect();
        if libraries.is_empty() {
            return Err("neither libEGL.so.1 nor libGL.so.1 could be loaded".into());
        }
        Ok(Self(libraries))
    }

    fn resolve(&self, name: &CStr) -> *mut c_void {
        type GetProc = unsafe extern "C" fn(*const c_char) -> *mut c_void;
        for library in &self.0 {
            for symbol in [b"eglGetProcAddress\0".as_slice(), b"glXGetProcAddressARB\0".as_slice()] {
                // SAFETY: these documented functions share this ABI; name is NUL-terminated.
                if let Ok(get_proc) = unsafe { library.get::<GetProc>(symbol) } {
                    let pointer = unsafe { get_proc(name.as_ptr()) };
                    if !pointer.is_null() {
                        return pointer;
                    }
                }
            }
        }
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn get_proc(context: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: mpv receives a stable Box<Resolver> pointer and passes a valid GL function name.
    let resolver = unsafe { &*context.cast::<Resolver>() };
    resolver.resolve(unsafe { CStr::from_ptr(name) })
}

struct GlState {
    render: RenderContext,
    get_integer: unsafe extern "C" fn(u32, *mut i32),
    _resolver: Box<Resolver>,
}

fn initialize(mpv: &Mpv, dirty: Arc<AtomicBool>) -> Result<GlState, String> {
    let resolver = Box::new(Resolver::new()?);
    let pointer = resolver.resolve(c"glGetIntegerv");
    if pointer.is_null() {
        return Err("glGetIntegerv is unavailable".into());
    }
    // SAFETY: GtkGLArea's render signal has made its context current. Resolver stays boxed
    // at this address until after RenderContext drops, and returns that context's GL functions.
    let mut render =
        unsafe { mpv.create_render_context_gl(get_proc, (&*resolver as *const Resolver).cast_mut().cast()) }.map_err(|e| e.to_string())?;
    render.set_update_callback(move || dirty.store(true, Ordering::Release));
    // SAFETY: this symbol is glGetIntegerv, with the documented OpenGL ABI.
    let get_integer = unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32, *mut i32)>(pointer) };
    Ok(GlState { render, get_integer, _resolver: resolver })
}

fn install(webview: gtk::Widget, mpv: Arc<Mpv>, video: PathBuf, app: tauri::AppHandle) -> Result<(), String> {
    let parent = webview.parent().ok_or("webview has no GTK parent")?;
    let container = parent.downcast::<gtk::Box>().map_err(|_| "expected Tauri's GtkBox parent")?;
    let overlay = gtk::Overlay::new();
    let area = gtk::GLArea::new();
    area.set_required_version(3, 3);
    area.set_auto_render(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    webview.set_halign(gtk::Align::Fill);
    webview.set_valign(gtk::Align::Fill);
    let context_errors = app.clone();
    area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            eprintln!("PROBE FAILED: GTK could not create a GL context: {error}");
            context_errors.exit(1);
        }
    });

    let state: Rc<RefCell<Option<GlState>>> = Rc::new(RefCell::new(None));
    let dirty = Arc::new(AtomicBool::new(true));
    let render_state = state.clone();
    let render_dirty = dirty.clone();
    let failed = Rc::new(std::cell::Cell::new(false));
    area.connect_render(move |area, _context| {
        if failed.get() {
            return gtk::glib::Propagation::Stop;
        }
        let draw = || -> Result<(), String> {
            if let Some(error) = area.error() {
                return Err(error.to_string());
            }
            if render_state.borrow().is_none() {
                let initialized = initialize(&mpv, render_dirty.clone())?;
                *render_state.borrow_mut() = Some(initialized);
                // Load only after context creation, on a worker. Blocking client commands on
                // the GL render thread can deadlock with mpv's video thread, even without
                // advanced control. This worker never accesses GTK or the render context.
                let loading_mpv = mpv.clone();
                let loading_video = video.clone();
                let load_errors = app.clone();
                std::thread::Builder::new()
                    .name("probe-load".into())
                    .spawn(move || {
                        let result = loading_video
                            .to_str()
                            .ok_or_else(|| "video path is not UTF-8".to_owned())
                            .and_then(|path| loading_mpv.loadfile(path, "").map_err(|e| e.to_string()));
                        match result {
                            Ok(()) => eprintln!("PROBE: local video load command completed"),
                            Err(error) => {
                                eprintln!("PROBE FAILED: {error}");
                                load_errors.exit(1);
                            }
                        }
                    })
                    .map_err(|e| e.to_string())?;
                eprintln!("PROBE: render context created");
            }
            let borrow = render_state.borrow();
            let current = borrow.as_ref().ok_or("render context missing")?;
            let mut fbo = 0;
            let mut viewport = [0; 4];
            // SAFETY: current GtkGLArea context and correctly sized outputs. GTK uses its own
            // framebuffer: do not assume FBO 0 or logical dimensions.
            unsafe {
                (current.get_integer)(0x8CA6, &mut fbo); // GL_DRAW_FRAMEBUFFER_BINDING
                (current.get_integer)(0x0BA2, viewport.as_mut_ptr()); // GL_VIEWPORT
                if viewport[2] > 0 && viewport[3] > 0 {
                    current.render.render_gl(fbo, viewport[2], viewport[3], true).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        };
        if let Err(error) = draw() {
            failed.set(true);
            eprintln!("PROBE FAILED: {error}");
            app.exit(1);
        }
        gtk::glib::Propagation::Stop
    });
    area.add_tick_callback(move |area, _clock| {
        if dirty.swap(false, Ordering::AcqRel) {
            area.queue_render();
        }
        gtk::glib::ControlFlow::Continue
    });
    area.connect_resize(|area, _, _| area.queue_render());
    area.connect_unrealize(move |area| {
        area.make_current();
        // RenderContext must be released before its GL context and loader disappear.
        drop(state.borrow_mut().take());
    });

    container.remove(&webview); // our Widget clone holds a reference during reparenting
    overlay.add(&area);
    overlay.add_overlay(&webview);
    container.pack_start(&overlay, true, true, 0);
    overlay.show_all();
    area.queue_render();
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .invoke_handler(tauri::generate_handler![probe_control, probe_status])
        .setup(|app| {
            let library = PathBuf::from(std::env::var("ANILILI_PROBE_LIBMPV")?);
            let video = PathBuf::from(std::env::var("ANILILI_PROBE_VIDEO")?);
            if !library.is_absolute() || !library.is_file() || !video.is_absolute() || !video.is_file() {
                return Err("set absolute paths to an LGPL libmpv .so and a local video".into());
            }
            let mpv = Arc::new(Mpv::new(
                &library,
                &[
                    ("vo", "libmpv"),
                    ("hwdec", "no"),
                    ("idle", "yes"),
                    ("keep-open", "yes"),
                    ("osc", "no"),
                    ("input-default-bindings", "no"),
                ],
            )?);
            mpv.request_log_messages("info")?;
            mpv.start_events(|event| eprintln!("MPV: {event:?}"));
            app.manage(Probe(mpv.clone()));
            let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Anilili Linux playback probe")
                .inner_size(1000.0, 650.0)
                .transparent(true)
                .build()?;
            let handle = app.handle().clone();
            let quitting = handle.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    quitting.exit(0);
                }
            });
            window.with_webview(move |platform| {
                let result = platform
                    .downcast_ref::<tauri_runtime_wry::Webview>()
                    .ok_or_else(|| "runtime is not Wry".to_owned())
                    .and_then(|native| install(native.inner().upcast(), mpv, video, handle.clone()));
                if let Err(error) = result {
                    eprintln!("PROBE FAILED: {error}");
                    handle.exit(1);
                }
            })?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("probe startup");
}
