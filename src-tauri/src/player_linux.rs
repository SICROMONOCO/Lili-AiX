//! Linux video: mpv's render API drawing into a GtkGLArea under the WebKitGTK webview.
//!
//! Windows hands mpv the window as `wid` and macOS uses a CAOpenGLLayer; Linux has neither
//! path, so the core runs with `vo=libmpv` and draws through the OpenGL render API into a
//! `GtkGLArea` reparented below Tauri's webview in a `GtkOverlay` (the arrangement Wry's
//! own `gtk_opengl` example demonstrates). The webview is transparent, so the React UI
//! floats over the video exactly as on the other platforms, and `video-margin-ratio-*`
//! moves the picture for the side panel. Proven first by
//! `experiments/linux-playback-probe`, then integrated here.
//!
//! GTK and GL work stays on the main thread: mpv's update callback only sets a dirty flag and
//! asks the main loop, once, for a render there. mpv client commands run on Tauri's command
//! threads, or on the player's own client thread when they come from a plain command
//! (`Player::off_ui`), never on the main thread (the render API forbids blocking client work
//! depending on the render thread).

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_void};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anilili_mpv::{Mpv, RenderContext, RenderCore};
use gtk::prelude::*;
use libloading::Library;

/// Keeps the GL loader libraries alive for the render context's life. The widget tree owns
/// the GLArea and render state; this handle is kept in `Player` so the pairing is explicit.
pub struct Video {
    _resolver: Arc<Resolver>,
    ready: Arc<AtomicBool>,
}

impl Video {
    /// Whether the GL render context exists yet. mpv drops a file loaded before this
    /// (`vo/libmpv` has nowhere to draw), so loads wait for it.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

// SAFETY: the resolver is only read through mpv's GL loader callback, which mpv calls with
// the GL context current; the libraries themselves are process-wide once loaded.
unsafe impl Send for Video {}
unsafe impl Sync for Video {}

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
    // SAFETY: mpv receives a stable Arc<Resolver> pointer and passes a valid GL name.
    let resolver = unsafe { &*context.cast::<Resolver>() };
    resolver.resolve(unsafe { CStr::from_ptr(name) })
}

thread_local! {
    /// The video's GLArea, for the render requests mpv's update callback posts to the main loop
    /// (a widget can't cross threads). Set and read on the main thread only.
    static AREA: RefCell<Option<gtk::GLArea>> = const { RefCell::new(None) };
}

/// The video's GL vendor, renderer and version, once its render context exists.
static GL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// What the video draws with, for Save diagnostics: the GPU driver decides much of how the video
/// and the webview over it are composited. `None` until the video layer first draws.
pub fn gl() -> Option<&'static str> {
    GL.get().map(String::as_str)
}

/// GL_VENDOR, GL_RENDERER and GL_VERSION of the current context.
fn gl_description(resolver: &Resolver) -> Option<String> {
    let pointer = resolver.resolve(c"glGetString");
    if pointer.is_null() {
        return None;
    }
    // SAFETY: this symbol is glGetString, with the documented OpenGL ABI.
    let get_string = unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32) -> *const c_char>(pointer) };
    let read = |name| {
        // SAFETY: a GL context is current (the GLArea's render signal); the result is a static
        // NUL-terminated string, or null.
        let text = unsafe { get_string(name) };
        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
    };
    // GL_VENDOR, GL_RENDERER, GL_VERSION.
    Some(format!("{}; {}; {}", read(0x1F00)?, read(0x1F01)?, read(0x1F02)?))
}

struct GlState {
    render: RenderContext,
    get_integer: unsafe extern "C" fn(u32, *mut i32),
    // Kept after `render` in drop order: the loader must outlive the render context
    // that resolves GL functions through it.
    _resolver: Arc<Resolver>,
}

fn initialize(mpv: &RenderCore, resolver: &Arc<Resolver>, dirty: Arc<AtomicBool>) -> Result<GlState, String> {
    let pointer = resolver.resolve(c"glGetIntegerv");
    if pointer.is_null() {
        return Err("glGetIntegerv is unavailable".into());
    }
    // SAFETY: GtkGLArea's render signal has made its context current. The resolver Arc is
    // cloned into the returned state, so it outlives the render context dropped with it.
    let mut render =
        unsafe { mpv.create_render_context_gl(get_proc, (Arc::as_ptr(resolver) as *mut Resolver).cast()) }.map_err(|e| e.to_string())?;
    render.set_update_callback(move || {
        // One render request in flight at a time; the render clears the flag.
        if !dirty.swap(true, Ordering::AcqRel) {
            gtk::glib::idle_add_once(|| {
                AREA.with(|area| {
                    if let Some(area) = area.borrow().as_ref() {
                        area.queue_render();
                    }
                })
            });
        }
    });
    // SAFETY: this symbol is glGetIntegerv, with the documented OpenGL ABI.
    let get_integer = unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32, *mut i32)>(pointer) };
    Ok(GlState { render, get_integer, _resolver: Arc::clone(resolver) })
}

/// Reparents `window`'s webview above a GLArea playing `mpv`. Call on the main thread,
/// after `Mpv::new` with `vo=libmpv`, before any file loads.
pub fn attach<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, mpv: &Mpv) -> Result<Video, Box<dyn std::error::Error>> {
    let resolver = Arc::new(Resolver::new()?);
    let ready = Arc::new(AtomicBool::new(false));
    let ready_in = Arc::clone(&ready);
    let mpv = mpv.render_core();
    // `with_webview` requires a Send closure, so the outcome crosses via Arc<Mutex>
    // (the callback itself runs on the main thread; this is only the return channel).
    let outcome: Arc<std::sync::Mutex<Option<Result<(), String>>>> = Arc::new(std::sync::Mutex::new(None));
    let sink = Arc::clone(&outcome);
    let resolver_in = Arc::clone(&resolver);
    window.with_webview(move |platform| {
        let result = platform
            .downcast_ref::<tauri_runtime_wry::Webview>()
            .ok_or_else(|| "runtime is not Wry".to_owned())
            .and_then(|native| install(native.inner().upcast(), &mpv, &resolver_in, &ready_in));
        *sink.lock().unwrap() = Some(result);
    })?;
    match outcome.lock().unwrap().take() {
        Some(Ok(())) => Ok(Video { _resolver: resolver, ready }),
        Some(Err(e)) => Err(e.into()),
        None => Err("webview platform callback did not run".into()),
    }
}

fn install(webview: gtk::Widget, mpv: &RenderCore, resolver: &Arc<Resolver>, ready: &Arc<AtomicBool>) -> Result<(), String> {
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
    area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            tracing::error!(%error, "GTK could not create a GL context");
        }
    });

    let mpv = mpv.clone();
    let resolver = Arc::clone(resolver);
    let state: Rc<RefCell<Option<GlState>>> = Rc::new(RefCell::new(None));
    let dirty = Arc::new(AtomicBool::new(false));
    let render_state = Rc::clone(&state);
    let render_dirty = Arc::clone(&dirty);
    let render_ready = Arc::clone(ready);
    area.connect_render(move |area, _context| {
        render_dirty.store(false, Ordering::Release);
        let draw = || -> Result<(), String> {
            if let Some(error) = area.error() {
                return Err(error.to_string());
            }
            if render_state.borrow().is_none() {
                let initialized = initialize(&mpv, &resolver, render_dirty.clone())?;
                *render_state.borrow_mut() = Some(initialized);
                render_ready.store(true, Ordering::Release);
                if let Some(gl) = gl_description(&resolver) {
                    tracing::info!(%gl, "mpv render context created (Linux)");
                    let _ = GL.set(gl);
                } else {
                    tracing::info!("mpv render context created (Linux)");
                }
            }
            let borrow = render_state.borrow();
            let current = borrow.as_ref().ok_or("render context missing")?;
            let mut fbo = 0;
            let mut viewport = [0; 4];
            // SAFETY: current GtkGLArea context and correctly sized outputs. GTK uses its
            // own framebuffer: do not assume FBO 0 or logical dimensions.
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
            tracing::error!(%error, "mpv render failed (Linux)");
        }
        gtk::glib::Propagation::Stop
    });
    // No tick callback: one keeps GTK's frame clock running at the display's rate for as long
    // as the window exists, video or not, and the whole window paid for it.
    AREA.with(|slot| *slot.borrow_mut() = Some(area.clone()));
    area.connect_resize(|area, _, _| area.queue_render());
    area.connect_unrealize(move |area| {
        AREA.with(|slot| slot.borrow_mut().take());
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
    tracing::info!("video layer attached (Linux)");
    Ok(())
}
