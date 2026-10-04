//! Keep the auth helper runnable while Android's browser owns the foreground.

use anyhow::Result;

pub(super) struct AuthLifetime;

impl AuthLifetime {
    pub(super) fn start() -> Result<Self> {
        #[cfg(target_os = "android")]
        set_active(true)?;
        Ok(Self)
    }
}

impl Drop for AuthLifetime {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        if let Err(error) = set_active(false) {
            bevy::log::warn!(%error, "could not stop Android authentication service");
        }
    }
}

#[cfg(target_os = "android")]
fn set_active(active: bool) -> Result<()> {
    crate::android::jni_call(|env, activity| {
        env.call_method(
            activity,
            "setAuthenticationActive",
            "(Z)V",
            &[jni::objects::JValue::Bool(u8::from(active))],
        )?;
        Ok(())
    })
}
