//! What Kaye asks of the phone, through its Java side in `android/java/KayeActivity.java`.
//!
//! Android has no file dialogs a program can just call. Its file picker belongs to another
//! app, and the answer comes back later to the activity. So a request here is sent to the
//! activity, and the thread that asked waits for `answer` to be called back from Java.
//! Only worker threads may wait like that: the window's own thread has to keep running, or
//! Android can't pause Kaye to show the picker at all.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};

use jni::JavaVM;
use jni::objects::{GlobalRef, JObject, JObjectArray, JString, JValue};
use jni::sys::jobject;

struct Activity {
    vm: JavaVM,
    activity: GlobalRef,
    files: PathBuf,
}

static ACTIVITY: OnceLock<Activity> = OnceLock::new();
static WAITING: Mutex<Option<HashMap<i64, mpsc::Sender<Option<Vec<String>>>>>> = Mutex::new(None);
static NEXT: AtomicI64 = AtomicI64::new(1);

/// Called once from `android_main`, with what `AndroidApp` hands over.
///
/// # Safety
/// `vm` and `activity` must be the live `JavaVM` and activity of this process.
pub unsafe fn init(vm: *mut c_void, activity: *mut c_void, files: PathBuf) {
    // SAFETY: the caller promises these are this process's VM and activity.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) }.expect("Android hands over a JavaVM");
    let activity = {
        let env = vm.attach_current_thread().expect("the main thread can attach to Java");
        // SAFETY: the activity pointer is a live reference for as long as Kaye runs.
        let local = unsafe { JObject::from_raw(activity as jobject) };
        env.new_global_ref(local).expect("the activity can be held")
    };
    let _ = ACTIVITY.set(Activity { vm, activity, files });
}

/// Where Kaye keeps its own files on the phone. Nothing else can see this folder.
pub fn files_dir() -> PathBuf {
    ACTIVITY.get().map_or_else(|| PathBuf::from("."), |a| a.files.clone())
}

/// Projects saved on the phone, which the home page lists.
pub fn projects_dir() -> PathBuf {
    files_dir().join("Kaye").join("Projects")
}

/// Where picked files are copied to, since Android only lends them out.
fn picked_dir() -> PathBuf {
    files_dir().join("picked")
}

/// Where an export is written before it's copied to the place the person picks.
pub fn export_dir() -> PathBuf {
    files_dir().join("export")
}

/// Clears out copies left from the last time, which nothing points at anymore.
pub fn tidy() {
    let _ = std::fs::remove_dir_all(picked_dir());
    let _ = std::fs::remove_dir_all(export_dir());
    let _ = std::fs::create_dir_all(projects_dir());
}

fn call(method: &str, signature: &str, args: impl FnOnce(&mut jni::JNIEnv) -> Vec<JObject<'static>>, extra: &[JValue]) -> bool {
    let Some(a) = ACTIVITY.get() else {
        return false;
    };
    let Ok(mut env) = a.vm.attach_current_thread() else {
        return false;
    };
    let objects = args(&mut env);
    let mut values: Vec<JValue> = extra.to_vec();
    values.extend(objects.iter().map(JValue::Object));
    let called = env.call_method(a.activity.as_obj(), method, signature, &values).is_ok();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
        return false;
    }
    called
}

fn java_string(env: &mut jni::JNIEnv, text: &str) -> JObject<'static> {
    // A local reference lives until this thread detaches, which is longer than the call.
    env.new_string(text).map(|s| unsafe { JObject::from_raw(s.into_raw()) }).unwrap_or_else(|_| JObject::null())
}

/// Sends a request and waits for its answer. `None` means it was cancelled.
fn ask(method: &str, signature: &str, strings: Vec<String>, flag: Option<bool>) -> Option<Vec<String>> {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    WAITING.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, tx);
    let mut extra = vec![JValue::Long(id)];
    if let Some(flag) = flag {
        extra.push(JValue::Bool(flag as u8));
    }
    let sent = call(method, signature, |env| strings.iter().map(|s| java_string(env, s)).collect(), &extra);
    if !sent {
        WAITING.lock().unwrap().get_or_insert_with(HashMap::new).remove(&id);
        return None;
    }
    rx.recv().ok().flatten()
}

/// Lets the person pick files with Android's own picker, and gives back copies of them.
/// Projects are copied into the projects folder, so saving them later keeps the work.
pub fn pick_files(multiple: bool, project: bool) -> Vec<PathBuf> {
    let dest = if project { projects_dir() } else { picked_dir() };
    let _ = std::fs::create_dir_all(&dest);
    ask("pickFiles", "(JZLjava/lang/String;)V", vec![dest.to_string_lossy().into_owned()], Some(multiple))
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Asks for a name in a small box, which brings up the phone's keyboard.
pub fn ask_name(title: &str, initial: &str) -> Option<String> {
    let answer = ask("askText", "(JZLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", vec![title.to_string(), initial.to_string(), "Save".to_string()], Some(false))?;
    let name = answer.into_iter().next()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Asks for text in a small box with the phone's keyboard, starting from `initial`. A
/// number gets the number keyboard.
pub fn ask_text(title: &str, initial: &str, numeric: bool) -> Option<String> {
    let answer = ask("askText", "(JZLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", vec![title.to_string(), initial.to_string(), "OK".to_string()], Some(numeric))?;
    answer.into_iter().next()
}

/// Puts text on the phone's clipboard, so it can be pasted in other apps.
pub fn set_clipboard(text: &str) -> bool {
    call("setClipboard", "(Ljava/lang/String;)V", |env| vec![java_string(env, text)], &[])
}

/// Lets the person choose where a finished file goes, and copies it there. This doesn't
/// wait, so the window's own thread may call it.
pub fn save_copy(path: &Path, name: &str) -> bool {
    let path = path.to_string_lossy().into_owned();
    call("saveCopy", "(Ljava/lang/String;Ljava/lang/String;)V", |env| vec![java_string(env, &path), java_string(env, name)], &[])
}

/// Opens a link in the phone's browser.
pub fn open_url(url: &str) -> bool {
    call("openUrl", "(Ljava/lang/String;)V", |env| vec![java_string(env, url)], &[])
}

/// A short message at the bottom of the screen, drawn by Android.
pub fn toast(text: &str) -> bool {
    call("toast", "(Ljava/lang/String;)V", |env| vec![java_string(env, text)], &[])
}

/// Called back from Java with the answer to a request. `answer` is null when it was
/// cancelled.
pub fn answer(env: &mut jni::JNIEnv, id: i64, answer: &JObjectArray) {
    let strings = (!answer.is_null()).then(|| {
        let count = env.get_array_length(answer).unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let item = env.get_object_array_element(answer, i).ok()?;
                let text: String = env.get_string(&JString::from(item)).ok()?.into();
                Some(text)
            })
            .collect::<Vec<_>>()
    });
    let waiting = WAITING.lock().unwrap().get_or_insert_with(HashMap::new).remove(&id);
    if let Some(tx) = waiting {
        let _ = tx.send(strings);
    }
}
