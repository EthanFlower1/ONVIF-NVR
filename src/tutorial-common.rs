/// macOS has a specific requirement that there must be a run loop running on the main thread in
/// order to open windows and use OpenGL, and that the global NSApplication instance must be
/// initialized.
/// On macOS this launches the callback function on a thread.
/// On other platforms it's just executed immediately.
#[cfg(not(target_os = "macos"))]
pub fn run<T, F: FnOnce() -> T + Send + 'static>(main: F) -> T
where
    T: Send + 'static,
{
    main()
}

#[cfg(target_os = "macos")]
pub fn run<T, F: FnOnce() -> T + Send + 'static>(main: F) -> T
where
    T: Send + 'static,
{
    use std::{
        ffi::c_void,
        sync::mpsc::{channel, Sender},
        thread,
    };

    use cocoa::{
        appkit::{NSApplication, NSWindow},
        base::{id, nil},
        foundation::NSPoint,
    };
    use objc::{
        class,
        declare,
        msg_send,
        runtime::{Object, Sel},
        sel, sel_impl,
    };
    use objc_foundation::NSString;

    unsafe {
        let app = cocoa::appkit::NSApp();
        let (send, recv) = channel::<()>();

        extern "C" fn on_finish_launching(this: &Object, _cmd: Sel, _notification: id) {
            let send = unsafe {
                let send_pointer = *this.get_ivar::<*const c_void>("send");
                let boxed = Box::from_raw(send_pointer as *mut Sender<()>);
                *boxed
            };
            send.send(()).unwrap();
        }

        // Create a delegate class
        let superclass = class!(NSObject);
        let mut delegate_class = objc::declare::ClassDecl::new("AppDelegate", superclass).unwrap();
        
        // Add instance variable
        delegate_class.add_ivar::<*const c_void>("send");
        
        // Add method
        extern "C" fn app_did_finish_launching(this: &Object, _sel: Sel, _notification: id) {
            unsafe {
                let send_ptr = *this.get_ivar::<*const c_void>("send");
                let sender = Box::from_raw(send_ptr as *mut Sender<()>);
                sender.send(()).unwrap();
                // Don't free the sender - we still need it
                std::mem::forget(sender);
            }
        }
        
        unsafe {
            delegate_class.add_method(
                sel!(applicationDidFinishLaunching:),
                app_did_finish_launching as extern "C" fn(&Object, Sel, id)
            );
        }
        
        let delegate_class = delegate_class.register();
        let delegate: id = unsafe { msg_send![delegate_class, new] };
        
        // Set the instance variable
        unsafe {
            let send_ptr = Box::into_raw(Box::new(send)) as *const c_void;
            (*delegate).set_ivar("send", send_ptr);
        }
        
        app.setDelegate_(delegate);

        let t = thread::spawn(move || {
            // Wait for the NSApp to launch to avoid possibly calling stop_() too early
            recv.recv().unwrap();

            let res = main();

            let app = cocoa::appkit::NSApp();
            app.stop_(cocoa::base::nil);

            // Stopping the event loop requires an actual event
            let event = cocoa::appkit::NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2_(
                nil,
                cocoa::appkit::NSEventType::NSApplicationDefined,
                NSPoint { x: 0.0, y: 0.0 },
                cocoa::appkit::NSEventModifierFlags::empty(),
                0.0,
                0,
                nil,
                cocoa::appkit::NSEventSubtype::NSApplicationActivatedEventType,
                0,
                0,
            );
            app.postEvent_atStart_(event, cocoa::base::YES);

            res
        });

        app.run();

        t.join().unwrap()
    }
}
