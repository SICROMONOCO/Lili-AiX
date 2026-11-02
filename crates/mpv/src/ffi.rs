//! Raw libmpv client API, resolved at runtime from the shared library.
//!
//! Loaded dynamically rather than linked: the app ships an LGPL libmpv and the
//! user must be able to replace it, and there is no import library to keep in
//! step. Layouts follow `client.h` of client API 2.x.

use std::ffi::{c_char, c_double, c_int, c_void};

use libloading::Library;

#[repr(C)]
pub struct MpvHandle {
    _private: [u8; 0],
}

pub type Format = c_int;
pub const FORMAT_NONE: Format = 0;
pub const FORMAT_STRING: Format = 1;
pub const FORMAT_FLAG: Format = 3;
pub const FORMAT_INT64: Format = 4;
pub const FORMAT_DOUBLE: Format = 5;

pub const ERROR_OPTION_NOT_FOUND: c_int = -5;

pub type EventId = c_int;
pub const EVENT_NONE: EventId = 0;
pub const EVENT_SHUTDOWN: EventId = 1;
pub const EVENT_LOG_MESSAGE: EventId = 2;
pub const EVENT_START_FILE: EventId = 6;
pub const EVENT_END_FILE: EventId = 7;
pub const EVENT_FILE_LOADED: EventId = 8;
pub const EVENT_VIDEO_RECONFIG: EventId = 17;
pub const EVENT_AUDIO_RECONFIG: EventId = 18;
pub const EVENT_SEEK: EventId = 20;
pub const EVENT_PLAYBACK_RESTART: EventId = 21;
pub const EVENT_PROPERTY_CHANGE: EventId = 22;

pub const END_FILE_REASON_EOF: c_int = 0;
pub const END_FILE_REASON_STOP: c_int = 2;
pub const END_FILE_REASON_QUIT: c_int = 3;
pub const END_FILE_REASON_ERROR: c_int = 4;
pub const END_FILE_REASON_REDIRECT: c_int = 5;

#[repr(C)]
pub struct Event {
    pub event_id: EventId,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct EventProperty {
    pub name: *const c_char,
    pub format: Format,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct EventEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct EventLogMessage {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
}

/// render.h: the render API, which hands mpv a GL context to draw into instead of a window.
#[repr(C)]
pub struct RenderContext {
    _private: [u8; 0],
}

pub type RenderParamType = c_int;
pub const RENDER_PARAM_INVALID: RenderParamType = 0;
pub const RENDER_PARAM_API_TYPE: RenderParamType = 1;
pub const RENDER_PARAM_OPENGL_INIT_PARAMS: RenderParamType = 2;
pub const RENDER_PARAM_OPENGL_FBO: RenderParamType = 3;
pub const RENDER_PARAM_FLIP_Y: RenderParamType = 4;

pub const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: RenderParamType = 12;

pub const RENDER_API_TYPE_OPENGL: &std::ffi::CStr = c"opengl";

pub const RENDER_UPDATE_FRAME: u64 = 1;

#[repr(C)]
pub struct RenderParam {
    pub kind: RenderParamType,
    pub data: *mut c_void,
}

pub type GetProcAddress = unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

/// `mpv_opengl_init_params` as of client API 2.0 (`extra_exts` is gone).
#[repr(C)]
pub struct OpenGlInitParams {
    pub get_proc_address: GetProcAddress,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct OpenGlFbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    /// 0 = unknown.
    pub internal_format: c_int,
}

pub type RenderUpdateFn = unsafe extern "C" fn(ctx: *mut c_void);

macro_rules! api {
    ($($name:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        /// Function table resolved from libmpv. The `Library` is kept alive
        /// alongside it, so the pointers stay valid for the table's lifetime.
        pub struct Api {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
            _library: Library,
        }

        impl Api {
            /// # Safety
            /// `library` must be a libmpv exporting client API 2.x.
            pub unsafe fn from_library(library: Library) -> Result<Self, libloading::Error> {
                unsafe {
                    Ok(Self {
                        $($name: *library.get(concat!(stringify!($name), "\0").as_bytes())?,)*
                        _library: library,
                    })
                }
            }
        }
    };
}

api! {
    mpv_client_api_version: fn() -> std::ffi::c_ulong;
    mpv_error_string: fn(c_int) -> *const c_char;
    mpv_free: fn(*mut c_void);
    mpv_create: fn() -> *mut MpvHandle;
    mpv_initialize: fn(*mut MpvHandle) -> c_int;
    mpv_terminate_destroy: fn(*mut MpvHandle);
    mpv_set_option_string: fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
    mpv_command: fn(*mut MpvHandle, *mut *const c_char) -> c_int;
    mpv_set_property: fn(*mut MpvHandle, *const c_char, Format, *mut c_void) -> c_int;
    mpv_set_property_string: fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
    mpv_get_property: fn(*mut MpvHandle, *const c_char, Format, *mut c_void) -> c_int;
    mpv_get_property_string: fn(*mut MpvHandle, *const c_char) -> *mut c_char;
    mpv_observe_property: fn(*mut MpvHandle, u64, *const c_char, Format) -> c_int;
    mpv_request_log_messages: fn(*mut MpvHandle, *const c_char) -> c_int;
    mpv_wait_event: fn(*mut MpvHandle, c_double) -> *mut Event;
    mpv_wakeup: fn(*mut MpvHandle);
    mpv_render_context_create: fn(*mut *mut RenderContext, *mut MpvHandle, *mut RenderParam) -> c_int;
    mpv_render_context_set_update_callback: fn(*mut RenderContext, Option<RenderUpdateFn>, *mut c_void);
    mpv_render_context_update: fn(*mut RenderContext) -> u64;
    mpv_render_context_render: fn(*mut RenderContext, *mut RenderParam) -> c_int;
    mpv_render_context_report_swap: fn(*mut RenderContext);
    mpv_render_context_free: fn(*mut RenderContext);
}
