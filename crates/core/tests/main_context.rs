//! Decoding must not need the default main context. In the app the GTK
//! main loop runs on it, and with glycin's `Auto` selector a decode in a
//! worker thread would be driven by that loop: if the main thread ever
//! waits on the worker, both hang. Core uses `MainContextSelector::Managed`.
//! This file holds one test, because the default context is per process.

mod common;

use std::sync::mpsc;
use std::time::Duration;

use safe_upload_core::Limits;
use safe_upload_core::decode::GlycinDecoder;

#[test]
fn decode_completes_while_the_default_main_loop_waits_on_it() {
    let jpeg = common::fixture("orient-1.jpg");
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let context = glib::MainContext::default();
        let main_loop = glib::MainLoop::new(Some(&context), false);
        // Like a UI thread that blocks on a worker: the loop dispatches
        // nothing else until the decode is done (or a minute passes).
        glib::idle_add_once(move || {
            started_tx.send(()).unwrap();
            let _ = done_rx.recv_timeout(Duration::from_secs(60));
        });
        main_loop.run();
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the main loop did not start");

    let (result_tx, result_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = GlycinDecoder.decode_bytes(jpeg, &Limits::default());
        let _ = result_tx.send(result.map(|d| d.pixels.dimensions()));
    });
    let result = result_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the decode waited on the busy default main context");
    done_tx.send(()).unwrap();
    assert_eq!(result.unwrap(), (48 * 2, 32 * 2));
}
