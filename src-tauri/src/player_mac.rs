//! macOS video: mpv's gpu-next renderer (libplacebo over Vulkan, through MoltenVK) presenting
//! into a `CAMetalLayer` under the webview, the renderer Windows uses.
//!
//! The layer backs an `NSView` placed directly **under** the WKWebView in the window's content
//! view (and above the window's glass, `glass.rs`). The webview doesn't draw its background
//! (`transparent(true)`), so the UI floats over the video exactly as on Windows, and the view
//! resizes with the window. `video-margin-ratio-*` moves the picture as there.
//!
//! mpv gets the layer itself as `wid` (`gpu-context=moltenvk`: MPVKit's context with our patch,
//! see `scripts/build-libmpv-macos.sh`), makes its Vulkan surface on it and presents from its
//! own thread; VideoToolbox frames reach it without a copy. This side only keeps the layer's
//! drawable at the view's size in pixels (mpv picks a change up within 50 ms) and stores the
//! screen's EDR headroom on the layer, from which mpv decides whether the display can take HDR
//! (`target-colorspace-hint=auto`). The view is hidden while nothing plays, so the window's glass
//! shows through the pages that let it.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string};
use objc2_quartz_core::{CALayer, CAMetalLayer};

/// Where mpv's MoltenVK context looks for the EDR headroom (the patch's `edr_headroom_key`).
const EDR_HEADROOM_KEY: &str = "mpvEDRHeadroom";

define_class!(
    // SAFETY: CAMetalLayer may be subclassed; nothing here needs Drop.
    #[unsafe(super(CAMetalLayer, CALayer))]
    #[name = "AniliiliVideoLayer"]
    struct VideoLayer;

    unsafe impl NSObjectProtocol for VideoLayer {}

    impl VideoLayer {
        /// MoltenVK sets a 1×1 drawable to force a presentation through, which flickers and
        /// can leave the drawable at 1×1 (mpv's own layer filters it the same way).
        #[unsafe(method(setDrawableSize:))]
        fn set_drawable_size(&self, size: NSSize) {
            if size.width > 1.0 && size.height > 1.0 {
                // SAFETY: the superclass's setter, same argument.
                let _: () = unsafe { msg_send![super(self), setDrawableSize: size] };
            }
        }

        /// MoltenVK switches EDR on from mpv's thread when an HDR swapchain starts, but the
        /// screen only enters EDR when it happens on the main thread.
        #[unsafe(method(setWantsExtendedDynamicRangeContent:))]
        fn set_wants_edr(&self, wants: bool) {
            if MainThreadMarker::new().is_some() {
                // SAFETY: the superclass's setter, same argument.
                let _: () = unsafe { msg_send![super(self), setWantsExtendedDynamicRangeContent: wants] };
            } else {
                // Not waiting: mpv's thread may itself be waited on by the main thread.
                let wants = NSNumber::new_bool(wants);
                // SAFETY: an NSObject method; it retains `self` and the argument until it runs.
                unsafe {
                    let _: () = msg_send![
                        self,
                        performSelectorOnMainThread: sel!(anililiApplyEDR:),
                        withObject: &*wants,
                        waitUntilDone: false
                    ];
                }
            }
        }

        #[unsafe(method(anililiApplyEDR:))]
        fn apply_edr(&self, wants: &NSNumber) {
            // SAFETY: the superclass's setter, called on the main thread.
            let _: () = unsafe { msg_send![super(self), setWantsExtendedDynamicRangeContent: wants.as_bool()] };
        }
    }
);

impl VideoLayer {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: CAMetalLayer's designated initialiser.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // The video is opaque; nothing under the layer needs compositing through it.
        this.setOpaque(true);
        this
    }

    /// Hands mpv the screen's EDR headroom (1 on an SDR screen).
    fn set_edr_headroom(&self, headroom: f64) {
        let value = NSNumber::new_f64(headroom);
        let key = NSString::from_str(EDR_HEADROOM_KEY);
        // SAFETY: CALayer takes values for any key (Key-Value Coding); mpv reads this one.
        unsafe {
            let _: () = msg_send![self, setValue: &*value, forKey: &*key];
        }
    }
}

struct ViewIvars {
    layer: Retained<VideoLayer>,
}

define_class!(
    // SAFETY: NSView may be subclassed; nothing here needs Drop.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "AniliiliVideoView"]
    #[ivars = ViewIvars]
    struct VideoView;

    unsafe impl NSObjectProtocol for VideoView {}

    impl VideoView {
        #[unsafe(method_id(makeBackingLayer))]
        fn make_backing_layer(&self) -> Retained<CALayer> {
            Retained::into_super(Retained::into_super(self.ivars().layer.clone()))
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: the superclass's implementation, same argument.
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.fit_drawable();
        }

        /// Keeps the layer at the screen's pixel density (Retina), also across screens.
        #[unsafe(method(viewDidChangeBackingProperties))]
        fn did_change_backing(&self) {
            self.fit_screen();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            self.fit_screen();
        }

        /// The window moved to another screen, or a screen changed (its HDR mode, say).
        #[unsafe(method(anililiScreenChanged:))]
        fn screen_changed(&self, _notification: &AnyObject) {
            self.fit_screen();
        }

        /// The webview above takes every click; the video never does.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut AnyObject {
            std::ptr::null_mut()
        }
    }
);

impl VideoView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars { layer: VideoLayer::new() });
        // SAFETY: NSView's designated initialiser.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this.observe_screens();
        this.fit_screen();
        this
    }

    /// The drawable is the view's size in pixels; mpv resizes its swapchain to follow it.
    fn fit_drawable(&self) {
        let size = self.convertSizeToBacking(self.bounds().size);
        self.ivars().layer.setDrawableSize(size);
    }

    /// The layer's scale and the EDR headroom of the screen the window is on.
    fn fit_screen(&self) {
        let layer = &self.ivars().layer;
        if let Some(window) = self.window() {
            layer.setContentsScale(window.backingScaleFactor());
            let headroom = window.screen().map_or(1.0, |screen| screen.maximumPotentialExtendedDynamicRangeColorComponentValue());
            layer.set_edr_headroom(headroom);
        }
        self.fit_drawable();
    }

    fn observe_screens(&self) {
        // SAFETY: the default centre; the observer is this view, which lives as long as the app
        // (and selector-based observers are weak since macOS 10.11); `anililiScreenChanged:`
        // takes the notification.
        unsafe {
            let center: Retained<NSObject> = msg_send![objc2::class!(NSNotificationCenter), defaultCenter];
            for name in
                [ns_string!("NSWindowDidChangeScreenNotification"), ns_string!("NSApplicationDidChangeScreenParametersNotification")]
            {
                let _: () = msg_send![
                    &*center,
                    addObserver: self,
                    selector: sel!(anililiScreenChanged:),
                    name: name,
                    object: std::ptr::null::<AnyObject>()
                ];
            }
        }
    }
}

/// The video view, which only the main thread may touch.
pub struct Video {
    view: Retained<VideoView>,
}

// SAFETY: `view` is only used through `Video::set_visible`, which `player.rs` calls on the main
// thread (`run_on_main_thread`), and dropped with the app's state at exit, on the main thread.
// `wid` only reads the layer's address.
unsafe impl Send for Video {}
unsafe impl Sync for Video {}

impl Video {
    /// The layer, as mpv's `wid`. It lives as long as this `Video`, which `Player` drops after
    /// the core.
    pub fn wid(&self) -> String {
        (Retained::as_ptr(&self.view.ivars().layer) as usize).to_string()
    }

    /// Shows the video while a file plays, and hides it while nothing does, so the window's
    /// glass shows through. Main thread only.
    pub fn set_visible(&self, visible: bool) {
        assert!(MainThreadMarker::new().is_some(), "Video::set_visible off the main thread");
        if self.view.isHidden() == visible {
            self.view.setHidden(!visible);
        }
    }
}

/// Creates the view under the webview of `window`, whose layer mpv draws into. Call on the main
/// thread, before `Mpv::new` (which takes [`Video::wid`]).
pub fn attach<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<Video, Box<dyn std::error::Error>> {
    let mtm = MainThreadMarker::new().ok_or("player_mac::attach must run on the main thread")?;
    let ns_window = window.ns_window()? as *mut objc2_app_kit::NSWindow;
    // SAFETY: Tauri hands out its live NSWindow, used here on the main thread only.
    let ns_window = unsafe { ns_window.as_ref() }.ok_or("main window has no NSWindow")?;
    let content = ns_window.contentView().ok_or("main window has no content view")?;
    let view = VideoView::new(mtm, content.bounds());
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    view.setHidden(true);

    // Directly under the webview: above any glass view Tauri put at the bottom, under the UI.
    let webview = content.subviews().iter().find(|v| is_kind_of(v, c"WKWebView"));
    content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, webview.as_deref());
    tracing::info!(found_webview = webview.is_some(), "video layer attached");
    Ok(Video { view })
}

fn is_kind_of(view: &NSView, class: &std::ffi::CStr) -> bool {
    objc2::runtime::AnyClass::get(class).is_some_and(|class| view.isKindOfClass(class))
}
