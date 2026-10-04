//! Android's NativeActivity entry point for the real client.

#[cfg(target_os = "android")]
#[bevy::prelude::bevy_main]
fn main() {
    bedrock_client::android::run();
}
