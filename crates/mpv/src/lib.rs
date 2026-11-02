//! Owned binding to libmpv.
//!
//! One [`Mpv`] owns one mpv core and the thread that drains its event queue.
//! libmpv's client API is thread-safe, so commands and property writes may come
//! from any thread while events are delivered on the event thread.
//!
//! Window embedding is just the `wid` option set before initialisation: on
//! Windows, passing the top-level window's HWND makes mpv paint that window's
//! surface, underneath any child windows (such as WebView2) that sit on it; on
//! macOS (`gpu-context=moltenvk`) it is the `CAMetalLayer` to present into.
//! Where an embedding needs a GL context instead (Linux), [`Mpv::create_render_context_gl`]
//! hands mpv one to draw into (`vo=libmpv`).

#[allow(dead_code)] // the table mirrors client.h, not just what we call today
mod ffi;

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::Serialize;

/// `MPV_ERROR_COMMAND`: the command was refused as given (e.g. `sub-add` with no file loaded).
pub const ERROR_COMMAND: i32 = -12;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not load libmpv: {0}")]
    Load(#[from] libloading::Error),
    #[error("libmpv client API {major}.{minor} is not supported (need 2.x)")]
    ApiVersion { major: u64, minor: u64 },
    #[error("mpv_create returned null")]
    Create,
    #[error("{context}: {message} ({code})")]
    Mpv { context: String, code: i32, message: String },
    #[error("string contains a NUL byte: {0:?}")]
    Nul(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Format to observe a property in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Flag,
    Int64,
    Double,
    /// Node-valued properties (`track-list`, `chapter-list`, …) arrive as JSON.
    String,
}

impl Format {
    fn raw(self) -> ffi::Format {
        match self {
            Format::Flag => ffi::FORMAT_FLAG,
            Format::Int64 => ffi::FORMAT_INT64,
            Format::Double => ffi::FORMAT_DOUBLE,
            Format::String => ffi::FORMAT_STRING,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PropertyValue {
    /// The property is unavailable (e.g. `duration` with nothing loaded).
    None,
    Flag(bool),
    Int64(i64),
    Double(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EndReason {
    Eof,
    Stop,
    Quit,
    Error,
    Redirect,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    StartFile,
    FileLoaded,
    PlaybackRestart,
    Seek,
    VideoReconfig,
    AudioReconfig,
    EndFile {
        reason: EndReason,
        /// mpv's error string when `reason` is `Error`.
        error: Option<String>,
        /// mpv's error number (negative) when `reason` is `Error`.
        code: Option<i32>,
    },
    PropertyChange {
        name: String,
        value: PropertyValue,
    },
    Log {
        prefix: String,
        level: String,
        text: String,
    },
    Shutdown,
}

struct Core {
    api: ffi::Api,
    handle: *mut ffi::MpvHandle,
}

// SAFETY: libmpv's client API is documented as thread-safe for a single
// handle; the handle is only destroyed in `Drop`, after the event thread exits.
unsafe impl Send for Core {}
unsafe impl Sync for Core {}

impl Drop for Core {
    fn drop(&mut self) {
        // SAFETY: last reference; nothing else can touch the handle now.
        unsafe { (self.api.mpv_terminate_destroy)(self.handle) }
    }
}

impl Core {
    fn check(&self, code: c_int, context: impl FnOnce() -> String) -> Result<()> {
        if code >= 0 {
            return Ok(());
        }
        Err(Error::Mpv { context: context(), code, message: self.error_string(code) })
    }

    fn error_string(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        unsafe { cstr_lossy((self.api.mpv_error_string)(code)) }
    }
}

/// Whether a version string mpv reports (`mpv-version`: `mpv v0.36.0`; `ffmpeg-version`: `6.0`,
/// `n7.1`) is at least `major.minor`. A git build without a release number (`N-118000-g…`, a
/// bare hash) counts as current.
pub fn version_at_least(text: &str, (major, minor): (u32, u32)) -> bool {
    text.split_whitespace()
        .find_map(|word| {
            let mut parts = word.trim_start_matches(['v', 'n']).split(['.', '-']).map(|p| p.parse::<u32>());
            match (parts.next(), parts.next()) {
                (Some(Ok(a)), Some(Ok(b))) => Some((a, b) >= (major, minor)),
                _ => None,
            }
        })
        .unwrap_or(true)
}

/// A running mpv core plus its event thread.
pub struct Mpv {
    core: Arc<Core>,
    observed: Arc<Mutex<HashMap<u64, String>>>,
    next_id: AtomicU64,
    stop: Arc<AtomicBool>,
    events: Mutex<Option<JoinHandle<()>>>,
    /// Whether `loadfile` takes an insertion index before its options (mpv 0.38 and later).
    loadfile_index: bool,
}

impl Mpv {
    /// Loads libmpv from `library`, applies `options` (these are where `wid`,
    /// `vo`, `hwdec` … go, since some only take effect before init), and
    /// initialises the core.
    pub fn new(library: &Path, options: &[(&str, &str)]) -> Result<Self> {
        // SAFETY: loading a library runs its initialisers; libmpv's are benign.
        let library = unsafe { load(library) }?;
        // SAFETY: symbols are checked by name; we verify the API major below.
        let api = unsafe { ffi::Api::from_library(library) }?;

        // SAFETY: plain getter.
        let version = unsafe { (api.mpv_client_api_version)() } as u64;
        let (major, minor) = (version >> 16, version & 0xffff);
        if major != 2 {
            return Err(Error::ApiVersion { major, minor });
        }

        // mpv_create returns null unless LC_NUMERIC is "C" (mpv parses and prints numbers with the C
        // library). GTK's init sets the whole locale from the environment, so on a desktop with,
        // say, en_US.UTF-8, every libmpv — bundled or the system's — refused to start.
        #[cfg(target_os = "linux")]
        // SAFETY: the string is NUL-terminated and static. Only LC_NUMERIC changes, and it is
        // set to "C" each time, so a thread reading it concurrently sees one value or the other.
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }

        // SAFETY: the only precondition, LC_NUMERIC being "C", holds (above; elsewhere nothing
        // changes it from the C default).
        let handle = unsafe { (api.mpv_create)() };
        if handle.is_null() {
            return Err(Error::Create);
        }
        let core = Arc::new(Core { api, handle });

        for (name, value) in options {
            let (c_name, c_value) = (cstring(name)?, cstring(value)?);
            // SAFETY: handle is live; strings outlive the call.
            let code = unsafe { (core.api.mpv_set_option_string)(core.handle, c_name.as_ptr(), c_value.as_ptr()) };
            // Builds differ (a Linux distribution's mpv can be years older than ours):
            // an option this one doesn't have yet is skipped, not fatal.
            if code == ffi::ERROR_OPTION_NOT_FOUND {
                log::warn!("libmpv has no option {name}; skipped");
                continue;
            }
            core.check(code, || format!("option {name}={value}"))?;
        }

        // SAFETY: handle is live and not yet initialised.
        let code = unsafe { (core.api.mpv_initialize)(core.handle) };
        core.check(code, || "mpv_initialize".into())?;

        let mut mpv = Self {
            core,
            observed: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            stop: Arc::new(AtomicBool::new(false)),
            events: Mutex::new(None),
            loadfile_index: true,
        };
        let release = mpv.get_string("mpv-version").unwrap_or_default();
        mpv.loadfile_index = version_at_least(&release, (0, 38));
        log::info!("libmpv {major}.{minor} ({release}) initialised");
        Ok(mpv)
    }

    /// Replaces the current file with `url`, with per-file `options` (`a=1,b=2`, may be empty).
    /// mpv 0.38 put an insertion index before the options; older builds take them
    /// third, and would read ours as an index.
    pub fn loadfile(&self, url: &str, options: &str) -> Result<()> {
        if self.loadfile_index {
            self.command(&["loadfile", url, "replace", "-1", options])
        } else {
            self.command(&["loadfile", url, "replace", options])
        }
    }

    /// Runs an mpv command, e.g. `["loadfile", url, "replace"]`.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        let owned = args.iter().map(|a| cstring(a)).collect::<Result<Vec<_>>>()?;
        let mut pointers: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: NULL-terminated array of valid C strings, alive for the call.
        let code = unsafe { (self.core.api.mpv_command)(self.core.handle, pointers.as_mut_ptr()) };
        self.core.check(code, || format!("command {}", args.join(" ")))
    }

    pub fn set_property(&self, name: &str, value: impl Into<SetValue>) -> Result<()> {
        let c_name = cstring(name)?;
        let api = &self.core.api;
        let handle = self.core.handle;
        // SAFETY: each arm passes a pointer to a value of the declared format,
        // alive for the duration of the call.
        let code = match value.into() {
            SetValue::String(s) => {
                let c_value = cstring(&s)?;
                unsafe { (api.mpv_set_property_string)(handle, c_name.as_ptr(), c_value.as_ptr()) }
            }
            SetValue::Flag(b) => {
                let mut v: c_int = b.into();
                unsafe { (api.mpv_set_property)(handle, c_name.as_ptr(), ffi::FORMAT_FLAG, ptr::from_mut(&mut v).cast()) }
            }
            SetValue::Int64(i) => {
                let mut v = i;
                unsafe { (api.mpv_set_property)(handle, c_name.as_ptr(), ffi::FORMAT_INT64, ptr::from_mut(&mut v).cast()) }
            }
            SetValue::Double(d) => {
                let mut v = d;
                unsafe { (api.mpv_set_property)(handle, c_name.as_ptr(), ffi::FORMAT_DOUBLE, ptr::from_mut(&mut v).cast()) }
            }
        };
        self.core.check(code, || format!("set {name}"))
    }

    pub fn get_double(&self, name: &str) -> Result<f64> {
        let c_name = cstring(name)?;
        let mut value = 0f64;
        // SAFETY: FORMAT_DOUBLE writes one f64 into `value`.
        let code = unsafe {
            (self.core.api.mpv_get_property)(self.core.handle, c_name.as_ptr(), ffi::FORMAT_DOUBLE, ptr::from_mut(&mut value).cast())
        };
        self.core.check(code, || format!("get {name}"))?;
        Ok(value)
    }

    pub fn get_string(&self, name: &str) -> Result<String> {
        let c_name = cstring(name)?;
        // SAFETY: returns an mpv-allocated string (or null), freed below.
        let raw = unsafe { (self.core.api.mpv_get_property_string)(self.core.handle, c_name.as_ptr()) };
        if raw.is_null() {
            return Err(Error::Mpv { context: format!("get {name}"), code: -8, message: "property unavailable".into() });
        }
        // SAFETY: raw is a valid C string owned by mpv until mpv_free.
        let value = unsafe { cstr_lossy(raw) };
        unsafe { (self.core.api.mpv_free)(raw.cast()) };
        Ok(value)
    }

    /// Subscribes to changes of `name`; they arrive as [`Event::PropertyChange`].
    pub fn observe(&self, name: &str, format: Format) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.observed.lock().unwrap().insert(id, name.to_owned());
        let c_name = cstring(name)?;
        // SAFETY: handle is live; name outlives the call (mpv copies it).
        let code = unsafe { (self.core.api.mpv_observe_property)(self.core.handle, id, c_name.as_ptr(), format.raw()) };
        self.core.check(code, || format!("observe {name}"))
    }

    /// Forwards mpv's own log at `level` (`"warn"`, `"info"`, …) as [`Event::Log`].
    pub fn request_log_messages(&self, level: &str) -> Result<()> {
        let c_level = cstring(level)?;
        // SAFETY: handle is live.
        let code = unsafe { (self.core.api.mpv_request_log_messages)(self.core.handle, c_level.as_ptr()) };
        self.core.check(code, || format!("request_log_messages {level}"))
    }

    /// Starts the event thread. Call once; `handler` runs on that thread.
    pub fn start_events(&self, mut handler: impl FnMut(Event) + Send + 'static) {
        let mut slot = self.events.lock().unwrap();
        assert!(slot.is_none(), "start_events called twice");
        let core = Arc::clone(&self.core);
        let observed = Arc::clone(&self.observed);
        let stop = Arc::clone(&self.stop);
        let thread = std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    // SAFETY: handle is live (we hold an Arc to the core). The
                    // returned event is valid until the next wait on this handle.
                    let event = unsafe { &*(core.api.mpv_wait_event)(core.handle, -1.0) };
                    if let Some(event) = translate(&core, &observed, event) {
                        let shutdown = matches!(event, Event::Shutdown);
                        handler(event);
                        if shutdown {
                            break;
                        }
                    }
                }
            })
            .expect("spawn mpv event thread");
        *slot = Some(thread);
    }

    /// A shareable handle to this core for creating a render context later, e.g. once a
    /// GTK GLArea has made its context current on the UI thread. The core itself stays
    /// owned by the original `Mpv`; this only keeps it alive.
    pub fn render_core(&self) -> RenderCore {
        RenderCore { core: Arc::clone(&self.core) }
    }

    /// Creates the render API's OpenGL context, for a core started with `vo=libmpv`. It must
    /// exist before a file with video loads (mpv otherwise disables the video track).
    ///
    /// # Safety
    /// The OpenGL context mpv will render into must be current on this thread, and stay the
    /// one current whenever [`RenderContext::render_gl`] runs. `get_proc_address` must resolve
    /// that context's GL functions, with `proc_ctx` as its first argument.
    pub unsafe fn create_render_context_gl(&self, get_proc_address: ffi::GetProcAddress, proc_ctx: *mut c_void) -> Result<RenderContext> {
        let mut init = ffi::OpenGlInitParams { get_proc_address, get_proc_address_ctx: proc_ctx };
        // No ADVANCED_CONTROL: it obliges a `mpv_render_context_update` after every callback,
        // and a hidden window (no draws) would then stall mpv's core.
        let mut params = [
            ffi::RenderParam { kind: ffi::RENDER_PARAM_API_TYPE, data: ffi::RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_OPENGL_INIT_PARAMS, data: ptr::from_mut(&mut init).cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        let mut ctx = ptr::null_mut();
        // SAFETY: params is INVALID-terminated and everything it points at outlives the call;
        // the caller guarantees the GL context.
        let code = unsafe { (self.core.api.mpv_render_context_create)(&mut ctx, self.core.handle, params.as_mut_ptr()) };
        self.core.check(code, || "mpv_render_context_create".into())?;
        Ok(RenderContext { core: Arc::clone(&self.core), ctx, update: None })
    }
}

/// A shareable handle to an mpv core, for creating its render context from another
/// (UI) thread once a GL context exists there. See [`Mpv::render_core`].
#[derive(Clone)]
pub struct RenderCore {
    core: Arc<Core>,
}

impl RenderCore {
    /// Same contract as [`Mpv::create_render_context_gl`].
    ///
    /// # Safety
    /// Same requirements as [`Mpv::create_render_context_gl`].
    pub unsafe fn create_render_context_gl(&self, get_proc_address: ffi::GetProcAddress, proc_ctx: *mut c_void) -> Result<RenderContext> {
        let mut init = ffi::OpenGlInitParams { get_proc_address, get_proc_address_ctx: proc_ctx };
        let mut params = [
            ffi::RenderParam { kind: ffi::RENDER_PARAM_API_TYPE, data: ffi::RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_OPENGL_INIT_PARAMS, data: ptr::from_mut(&mut init).cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        let mut ctx = ptr::null_mut();
        // SAFETY: params is INVALID-terminated and everything it points at outlives the call;
        // the caller guarantees the GL context.
        let code = unsafe { (self.core.api.mpv_render_context_create)(&mut ctx, self.core.handle, params.as_mut_ptr()) };
        self.core.check(code, || "mpv_render_context_create".into())?;
        Ok(RenderContext { core: Arc::clone(&self.core), ctx, update: None })
    }
}

/// mpv's render API context: frames are drawn by calling [`RenderContext::render_gl`] with
/// the target framebuffer, instead of mpv owning a window.
pub struct RenderContext {
    /// Keeps the core alive: the render context must be freed before the core is destroyed.
    core: Arc<Core>,
    ctx: *mut ffi::RenderContext,
    update: Option<Box<UpdateFn>>,
}

type UpdateFn = Box<dyn Fn() + Send + Sync>;

// SAFETY: mpv's render context locks internally; only `render_gl`/`report_swap` need the GL
// context, which their callers guarantee.
unsafe impl Send for RenderContext {}
unsafe impl Sync for RenderContext {}

unsafe extern "C" fn on_render_update(data: *mut c_void) {
    // SAFETY: `data` is the boxed callback set in `set_update_callback`, alive until it is
    // replaced or the context is freed, both of which unregister it first.
    let callback = unsafe { &*(data as *const UpdateFn) };
    callback();
}

impl RenderContext {
    /// Called (from any mpv thread) whenever a new frame is ready or a redraw is wanted. It must
    /// not call back into mpv; it should only schedule a draw.
    pub fn set_update_callback(&mut self, callback: impl Fn() + Send + Sync + 'static) {
        // SAFETY: unregister before the old box drops.
        unsafe { (self.core.api.mpv_render_context_set_update_callback)(self.ctx, None, ptr::null_mut()) };
        let boxed: Box<UpdateFn> = Box::new(Box::new(callback));
        let data = ptr::from_ref::<UpdateFn>(&boxed).cast_mut().cast();
        // SAFETY: the box lives in `self.update` until replaced or dropped.
        unsafe { (self.core.api.mpv_render_context_set_update_callback)(self.ctx, Some(on_render_update), data) };
        self.update = Some(boxed);
    }

    /// Whether a new frame should be drawn (since the last call).
    pub fn update(&self) -> bool {
        // SAFETY: ctx is live.
        unsafe { (self.core.api.mpv_render_context_update)(self.ctx) & ffi::RENDER_UPDATE_FRAME != 0 }
    }

    /// Draws the current frame into framebuffer `fbo` of `width`×`height` pixels.
    ///
    /// # Safety
    /// The GL context the render context was created with must be current on this thread.
    pub unsafe fn render_gl(&self, fbo: i32, width: i32, height: i32, flip_y: bool) -> Result<()> {
        let mut target = ffi::OpenGlFbo { fbo, w: width, h: height, internal_format: 0 };
        let mut flip: c_int = flip_y.into();
        // Draw what is due now, rather than holding the caller's (display link) thread until
        // the frame's target time.
        let mut block: c_int = 0;
        let mut params = [
            ffi::RenderParam { kind: ffi::RENDER_PARAM_OPENGL_FBO, data: ptr::from_mut(&mut target).cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_FLIP_Y, data: ptr::from_mut(&mut flip).cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_BLOCK_FOR_TARGET_TIME, data: ptr::from_mut(&mut block).cast() },
            ffi::RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        // SAFETY: params is INVALID-terminated and outlives the call; the GL context is current.
        let code = unsafe { (self.core.api.mpv_render_context_render)(self.ctx, params.as_mut_ptr()) };
        self.core.check(code, || "mpv_render_context_render".into())
    }

    /// Tells mpv the drawn frame reached the screen (for its frame timing).
    pub fn report_swap(&self) {
        // SAFETY: ctx is live.
        unsafe { (self.core.api.mpv_render_context_report_swap)(self.ctx) }
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        // SAFETY: ctx is live; freeing unregisters the callback before `self.update` drops.
        // The core outlives this (we hold an Arc).
        unsafe { (self.core.api.mpv_render_context_free)(self.ctx) }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // SAFETY: handle is live until the core's own Drop.
        unsafe { (self.core.api.mpv_wakeup)(self.core.handle) };
        if let Some(thread) = self.events.lock().unwrap().take() {
            let _ = thread.join();
        }
        // The core (and mpv_terminate_destroy) goes with the last Arc.
    }
}

pub enum SetValue {
    String(String),
    Flag(bool),
    Int64(i64),
    Double(f64),
}

impl From<&str> for SetValue {
    fn from(v: &str) -> Self {
        SetValue::String(v.to_owned())
    }
}
impl From<String> for SetValue {
    fn from(v: String) -> Self {
        SetValue::String(v)
    }
}
impl From<bool> for SetValue {
    fn from(v: bool) -> Self {
        SetValue::Flag(v)
    }
}
impl From<i64> for SetValue {
    fn from(v: i64) -> Self {
        SetValue::Int64(v)
    }
}
impl From<f64> for SetValue {
    fn from(v: f64) -> Self {
        SetValue::Double(v)
    }
}

/// Loads libmpv. On Windows its imports are looked for in its own folder first, then the
/// system's: the app ships the Vulkan loader (`vulkan-1.dll`) beside it, because libmpv imports
/// from it directly and a PC's own copy can be missing or too old ("Entry Point Not Found").
///
/// # Safety
/// Loading a library runs its initialisers.
unsafe fn load(library: &Path) -> std::result::Result<libloading::Library, libloading::Error> {
    #[cfg(windows)]
    {
        use libloading::os::windows::{LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, Library};
        // The folder search needs a clean absolute path: given the dev path
        // (`…\src-tauri/../vendor/…`), Windows silently took System32's `vulkan-1.dll` instead.
        let library = std::fs::canonicalize(library).unwrap_or_else(|_| library.to_path_buf());
        // SAFETY: as the caller's.
        unsafe { Library::load_with_flags(&library, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }.map(Into::into)
    }
    #[cfg(not(windows))]
    // SAFETY: as the caller's.
    unsafe {
        libloading::Library::new(library)
    }
}

fn translate(core: &Core, observed: &Mutex<HashMap<u64, String>>, event: &ffi::Event) -> Option<Event> {
    Some(match event.event_id {
        ffi::EVENT_NONE => return None,
        ffi::EVENT_SHUTDOWN => Event::Shutdown,
        ffi::EVENT_START_FILE => Event::StartFile,
        ffi::EVENT_FILE_LOADED => Event::FileLoaded,
        ffi::EVENT_PLAYBACK_RESTART => Event::PlaybackRestart,
        ffi::EVENT_SEEK => Event::Seek,
        ffi::EVENT_VIDEO_RECONFIG => Event::VideoReconfig,
        ffi::EVENT_AUDIO_RECONFIG => Event::AudioReconfig,
        ffi::EVENT_END_FILE => {
            // SAFETY: END_FILE carries an mpv_event_end_file.
            let data = unsafe { &*(event.data as *const ffi::EventEndFile) };
            let reason = match data.reason {
                ffi::END_FILE_REASON_EOF => EndReason::Eof,
                ffi::END_FILE_REASON_STOP => EndReason::Stop,
                ffi::END_FILE_REASON_QUIT => EndReason::Quit,
                ffi::END_FILE_REASON_ERROR => EndReason::Error,
                ffi::END_FILE_REASON_REDIRECT => EndReason::Redirect,
                _ => EndReason::Unknown,
            };
            let failed = reason == EndReason::Error;
            let error = failed.then(|| core.error_string(data.error));
            Event::EndFile { reason, error, code: failed.then_some(data.error) }
        }
        ffi::EVENT_PROPERTY_CHANGE => {
            // SAFETY: PROPERTY_CHANGE carries an mpv_event_property.
            let data = unsafe { &*(event.data as *const ffi::EventProperty) };
            let name = observed
                .lock()
                .unwrap()
                .get(&event.reply_userdata)
                .cloned()
                // SAFETY: name is a valid C string for the event's lifetime.
                .unwrap_or_else(|| unsafe { cstr_lossy(data.name) });
            // SAFETY: `data.data` points at a value of `data.format`.
            let value = unsafe { read_value(data.format, data.data) };
            Event::PropertyChange { name, value }
        }
        ffi::EVENT_LOG_MESSAGE => {
            // SAFETY: LOG_MESSAGE carries an mpv_event_log_message.
            let data = unsafe { &*(event.data as *const ffi::EventLogMessage) };
            // SAFETY: all three are valid C strings for the event's lifetime.
            unsafe {
                Event::Log {
                    prefix: cstr_lossy(data.prefix),
                    level: cstr_lossy(data.level),
                    text: cstr_lossy(data.text).trim_end().to_owned(),
                }
            }
        }
        _ => return None,
    })
}

/// # Safety
/// `data` must point at a value of `format`, or be null for `FORMAT_NONE`.
unsafe fn read_value(format: ffi::Format, data: *mut c_void) -> PropertyValue {
    if data.is_null() {
        return PropertyValue::None;
    }
    unsafe {
        match format {
            ffi::FORMAT_FLAG => PropertyValue::Flag(*(data as *const c_int) != 0),
            ffi::FORMAT_INT64 => PropertyValue::Int64(*(data as *const i64)),
            ffi::FORMAT_DOUBLE => PropertyValue::Double(*(data as *const f64)),
            ffi::FORMAT_STRING => PropertyValue::String(cstr_lossy(*(data as *const *const c_char))),
            _ => PropertyValue::None,
        }
    }
}

/// # Safety
/// `ptr` must be null or a valid NUL-terminated string.
unsafe fn cstr_lossy(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Nul(s.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::version_at_least;

    #[test]
    fn reads_versions() {
        assert!(!version_at_least("mpv v0.36.0", (0, 38)));
        assert!(!version_at_least("mpv 0.36.0", (0, 38)));
        assert!(version_at_least("mpv v0.38.0", (0, 38)));
        assert!(version_at_least("mpv v0.40.0-dev-g413ff0b1cd", (0, 38)));
        assert!(version_at_least("mpv 413ff0b1cd", (0, 38)));
        assert!(version_at_least("", (0, 38)));
        assert!(!version_at_least("6.0", (7, 0)));
        assert!(version_at_least("n7.1", (7, 0)));
        assert!(version_at_least("9.0.1", (7, 0)));
        assert!(version_at_least("N-118000-g1234abcd", (7, 0)));
    }
}
