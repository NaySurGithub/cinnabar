//! JNI calls stay off Android's main thread; Java posts UI operations to that thread.

use std::{cell::Cell, ffi::c_void, path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result, bail};
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{GlobalRef, JObject, JString, JValue},
    sys::{JNI_ERR, JNI_VERSION_1_6, jint},
};

use super::{
    Paths,
    bootstrap::{Attempt, Coordinator},
};

static PATHS: OnceLock<Paths> = OnceLock::new();
static BOOTSTRAP_VM: OnceLock<JavaVM> = OnceLock::new();
static BOOTSTRAP: Coordinator<GlobalRef> = Coordinator::new();
thread_local! { static IN_BOOTSTRAP: Cell<bool> = const { Cell::new(false) }; }

pub(super) fn paths() -> Result<Paths> {
    if let Some(paths) = PATHS.get() {
        return Ok(paths.clone());
    }
    // Android may restore NativeActivity after killing the process; its private carriers persist.
    let paths = jni_call(query_paths)?;
    let _ = PATHS.set(paths.clone());
    Ok(paths)
}

pub(crate) fn jni_call<T>(
    call: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> Result<T>,
) -> Result<T> {
    let bootstrap = BOOTSTRAP.owner();
    if let Some(activity) =
        bootstrap.filter(|_| IN_BOOTSTRAP.get() || bevy::android::ANDROID_APP.get().is_none())
    {
        let vm = BOOTSTRAP_VM
            .get()
            .context("bootstrap JVM is not available")?;
        let mut env = vm.attach_current_thread()?;
        let result = env.with_local_frame(16, |env| call(env, activity.as_obj()));
        if result.is_err() && env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        return result;
    }
    let app = bevy::android::ANDROID_APP
        .get()
        .context("Android Activity is not available")?;
    // AndroidApp owns both pointers for the lifetime of this native thread.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let borrowed = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = env.with_local_frame(16, |env| call(env, &borrowed));
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

pub(super) fn string_call(method: &str, value: &str) -> Result<()> {
    jni_call(|env, activity| {
        let value = JObject::from(env.new_string(value)?);
        env.call_method(
            activity,
            method,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&value)],
        )?;
        Ok(())
    })
}

pub(crate) fn strings_call(method: &str, first: &str, second: &str) -> Result<()> {
    jni_call(|env, activity| {
        let first = JObject::from(env.new_string(first)?);
        let second = JObject::from(env.new_string(second)?);
        env.call_method(
            activity,
            method,
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&first), JValue::Object(&second)],
        )?;
        Ok(())
    })
}

pub(crate) fn consent_state() -> Result<i32> {
    jni_call(|env, activity| Ok(env.call_method(activity, "consentState", "()I", &[])?.i()?))
}

pub(crate) fn progress(message: &str) {
    let _ = string_call("showProgress", message);
}

fn directory(env: &mut JNIEnv<'_>, activity: &JObject<'_>, method: &str) -> Result<PathBuf> {
    let file = env
        .call_method(activity, method, "()Ljava/io/File;", &[])?
        .l()?;
    let value = JString::from(
        env.call_method(file, "getAbsolutePath", "()Ljava/lang/String;", &[])?
            .l()?,
    );
    let path: String = env.get_string(&value)?.into();
    Ok(PathBuf::from(path))
}

fn query_paths(env: &mut JNIEnv<'_>, activity: &JObject<'_>) -> Result<Paths> {
    let files_dir = directory(env, activity, "getFilesDir")?;
    let cache_dir = directory(env, activity, "getCacheDir")?;
    let info = env
        .call_method(
            activity,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            &[],
        )?
        .l()?;
    let value = JString::from(
        env.get_field(info, "nativeLibraryDir", "Ljava/lang/String;")?
            .l()?,
    );
    let native_library_dir = PathBuf::from(String::from(env.get_string(&value)?));
    Ok(Paths {
        resources_dir: files_dir.join("resources"),
        files_dir,
        cache_dir,
        native_library_dir,
    })
}

fn initialise(env: &mut JNIEnv<'_>, activity: &JObject<'_>) -> Result<()> {
    let paths = query_paths(env, activity)?;
    if let Some(previous) = PATHS.get() {
        if previous != &paths {
            bail!("Android storage changed during bootstrap");
        }
    } else {
        PATHS
            .set(paths)
            .map_err(|_| anyhow::anyhow!("Android paths already configured"))?;
    }
    let _ = BOOTSTRAP_VM.set(env.get_java_vm()?);
    Ok(())
}

fn resources(attempt: &Attempt<'_, GlobalRef>) -> Result<()> {
    let archive = jni_call(|env, activity| {
        let name = JObject::from(env.new_string(super::runtime().resource_archive)?);
        let value = JString::from(
            env.call_method(
                activity,
                "stageResourceArchive",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&name)],
            )?
            .l()?,
        );
        Ok(PathBuf::from(String::from(env.get_string(&value)?)))
    })?;
    let paths = paths()?;
    let staged = attempt.fresh_staging(&paths.resources_dir)?;
    super::archive::extract(&archive, &staged, BOOTSTRAP.cancellation())?;
    let _ = std::fs::remove_dir_all(&paths.resources_dir);
    std::fs::rename(&staged, &paths.resources_dir)?;
    let _ = std::fs::remove_file(archive);
    // stage_kit expects each of these directories, even when no scripts need shipping.
    std::fs::create_dir_all(paths.resources_dir.join("prep-kit/scripts"))?;
    launcher::install_layout::configure_android(
        paths.files_dir,
        paths.native_library_dir,
        paths.resources_dir,
    )?;
    Ok(())
}

extern "system" fn prepare_native(mut env: JNIEnv<'_>, activity: JObject<'_>) {
    let owner = match env.new_global_ref(&activity) {
        Ok(owner) => owner,
        Err(error) => {
            eprintln!("Android bootstrap Activity is unavailable: {error}");
            return;
        }
    };
    let attempt = BOOTSTRAP.begin(owner, || {
        eprintln!("Android setup is waiting for the previous attempt");
    });
    IN_BOOTSTRAP.set(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<bool> {
        if env.call_method(&activity, "isDestroyed", "()Z", &[])?.z()?
            || env.call_method(&activity, "isFinishing", "()Z", &[])?.z()?
        {
            return Ok(false);
        }
        initialise(&mut env, &activity)?;
        resources(&attempt)?;
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build_global();
        crate::first_run::android::bootstrap(BOOTSTRAP.cancellation())
    }))
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "resource setup panicked; see the application log"
        ))
    });
    let (ready, message) = match result {
        Ok(ready) => (ready, String::new()),
        Err(error) => {
            let message = format!("{error:#}");
            eprintln!("Android setup failed: {message}");
            (false, message)
        }
    };
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    let _ = (|| -> Result<()> {
        let message = JObject::from(env.new_string(message)?);
        env.call_method(
            &activity,
            "setupComplete",
            "(ZLjava/lang/String;)V",
            &[JValue::Bool(u8::from(ready)), JValue::Object(&message)],
        )?;
        Ok(())
    })();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    IN_BOOTSTRAP.set(false);
}

extern "system" fn cancel_native(env: JNIEnv<'_>, activity: JObject<'_>) {
    BOOTSTRAP.cancel(|owner| {
        env.is_same_object(owner.as_obj(), &activity)
            .unwrap_or(false)
    });
}

/// Registered by class name from the runtime manifest, avoiding package-specific JNI symbols.
///
/// # Safety
/// Android must supply the live VM pointer while loading this native library.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn JNI_OnLoad(vm: *mut jni::sys::JavaVM, _: *mut c_void) -> jint {
    let result = (|| -> Result<()> {
        let vm = unsafe { JavaVM::from_raw(vm) }?;
        let mut env = vm.get_env()?;
        let class = format!(
            "{}/BootstrapActivity",
            super::runtime().application_id.replace('.', "/")
        );
        env.register_native_methods(
            class,
            &[
                NativeMethod {
                    name: "prepareNative".into(),
                    sig: "()V".into(),
                    fn_ptr: prepare_native as *mut c_void,
                },
                NativeMethod {
                    name: "cancelNative".into(),
                    sig: "()V".into(),
                    fn_ptr: cancel_native as *mut c_void,
                },
            ],
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Android JNI registration failed: {error:#}");
        JNI_ERR
    } else {
        JNI_VERSION_1_6
    }
}

pub(super) fn show_failure(message: &str) -> Result<()> {
    strings_call("showFailure", launcher::PRODUCT_NAME, message)
}
