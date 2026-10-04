//! Uses the activity's ClipboardManager; the JSON UI keeps ownership of editing.

use jni::{
    JNIEnv, JavaVM,
    objects::{JObject, JString, JValue},
};

fn with_clipboard<T>(
    access: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let app = bevy::android::ANDROID_APP
        .get()
        .ok_or("Android activity is not initialized")?;
    // The app owns the VM and global activity reference for this entire access.
    let vm =
        unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|error| error.to_string())?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| error.to_string())?;
    let result = env.with_local_frame(16, |env| {
        // JObject does not delete the borrowed global reference on drop.
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let service_name = env.new_string("clipboard")?;
        let clipboard = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&service_name)],
            )?
            .l()?;
        access(env, &activity, &clipboard)
    });
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result.map_err(|error: jni::errors::Error| error.to_string())
}

pub(super) fn read_text() -> Result<Option<String>, String> {
    with_clipboard(|env, _, clipboard| {
        let clip = env
            .call_method(
                clipboard,
                "getPrimaryClip",
                "()Landroid/content/ClipData;",
                &[],
            )?
            .l()?;
        if clip.is_null() || env.call_method(&clip, "getItemCount", "()I", &[])?.i()? == 0 {
            return Ok(None);
        }
        let item = env
            .call_method(
                &clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[JValue::Int(0)],
            )?
            .l()?;
        let text = env
            .call_method(&item, "getText", "()Ljava/lang/CharSequence;", &[])?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }
        let text = JString::from(
            env.call_method(&text, "toString", "()Ljava/lang/String;", &[])?
                .l()?,
        );
        Ok(Some(env.get_string(&text)?.into()))
    })
}

pub(super) fn write_text(text: &str) -> Result<(), String> {
    with_clipboard(|env, _, clipboard| {
        let label = env.new_string("")?;
        let text = env.new_string(text)?;
        let clip = env
            .call_static_method(
                "android/content/ClipData",
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[JValue::Object(&label), JValue::Object(&text)],
            )?
            .l()?;
        env.call_method(
            clipboard,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[JValue::Object(&clip)],
        )?;
        Ok(())
    })
}
