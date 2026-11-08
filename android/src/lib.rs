//! Where Android starts Kaye. It's `src/main.rs` for a phone: the window is the whole
//! screen, and there's no updater or file association to set up.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{Backends, Instance, InstanceDescriptor};
use jni::JNIEnv;
use jni::objects::{JClass, JObjectArray};
use jni::sys::jlong;
use kaye::settings::{GraphicsApi, Settings, StartupMarker};

fn backends_for(api: GraphicsApi) -> Option<Backends> {
    match api {
        GraphicsApi::Vulkan => Some(Backends::VULKAN),
        GraphicsApi::OpenGl => Some(Backends::GL),
        _ => None,
    }
}

fn has_adapter(backends: Backends) -> bool {
    let mut descriptor = InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backends;
    !pollster_block(Instance::new(descriptor).enumerate_adapters(backends)).is_empty()
}

/// eframe already depends on pollster, but doesn't hand it out, so this is its one line.
fn pollster_block<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

fn wgpu_options(settings: &Settings, backends: Option<Backends>) -> WgpuConfiguration {
    let mut options = WgpuConfiguration::default();
    options.surface.present_mode = if settings.system.vsync {
        eframe::wgpu::PresentMode::AutoVsync
    } else {
        eframe::wgpu::PresentMode::AutoNoVsync
    };
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        // A phone's battery matters more than the last few frames.
        setup.power_preference = if settings.system.high_performance_gpu {
            eframe::wgpu::PowerPreference::HighPerformance
        } else {
            eframe::wgpu::PowerPreference::LowPower
        };
        if let Some(backends) = backends {
            setup.instance_descriptor.backends = backends;
        }
    }
    options
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("Kaye"));
    std::panic::set_hook(Box::new(|info| log::error!("Kaye stopped: {info}")));

    let files = app.internal_data_path().unwrap_or_else(|| std::path::PathBuf::from("/data/local/tmp"));
    // Settings, backups and downloads are kept where Windows keeps them, under APPDATA, so
    // pointing that at the app's own folder puts them all there.
    // SAFETY: nothing else is running yet to read the environment at the same time.
    unsafe {
        std::env::set_var("APPDATA", &files);
        std::env::set_var("TMPDIR", files.join("tmp"));
    }
    let _ = std::fs::create_dir_all(files.join("tmp"));
    // SAFETY: these are the VM and activity Android started this process with.
    unsafe { kaye::android::init(app.vm_as_ptr(), app.activity_as_ptr(), files) };
    kaye::android::tidy();

    let mut settings = Settings::load();
    let marker = StartupMarker::path();
    let chosen = backends_for(settings.system.graphics_api);
    let usable = chosen.filter(|b| !marker.left_behind() && has_adapter(*b));
    let graphics_failed = (chosen.is_some() && usable.is_none()).then_some(settings.system.graphics_api);
    if graphics_failed.is_some() {
        settings.system.graphics_api = GraphicsApi::Auto;
        settings.save();
    }
    marker.set();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Kaye"),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(&settings, usable),
        android_app: Some(app),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Kaye",
        options,
        Box::new(move |cc| Ok(Box::new(kaye::app::KayeApp::new(cc, Vec::new(), settings, graphics_failed)))),
    );
    if let Err(error) = result {
        log::error!("Kaye couldn't start: {error}");
    }
}

/// The answer to a request from `kaye::android`, called from Java.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_toorium_kaye_KayeActivity_answer(mut env: JNIEnv, _class: JClass, id: jlong, answer: JObjectArray) {
    kaye::android::answer(&mut env, id, &answer);
}
